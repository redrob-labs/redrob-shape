//! wgpu viewport: camera, grid, picking, shading modes.
//!
//! [`Viewport`] renders a [`Scene`] into its own colour+depth textures with one solid pipeline
//! (flat Lambert) and one line pipeline (grid, wire overlay, selection outline) that draws each
//! segment as a screen-space quad. The UI shows the
//! colour texture as an image; rendering is submitted on the queue directly, so it does not depend
//! on egui's own render pass (which has no depth buffer). Picking is done on the CPU with the
//! geometry BVH -- exact, and it does not need a GPU read-back.

pub mod camera;
mod geometry;

pub use camera::OrbitCamera;
pub use geometry::{ShadingMode, build_frame, pick};

use bytemuck::{Pod, Zeroable};
use shape_math::Vec3;
use shape_scene::Scene;

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-gpu";

/// One vertex of either pipeline.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

impl Vertex {
    pub fn new(position: Vec3, normal: Vec3, color: [f32; 4]) -> Self {
        Self { position: position.to_array(), normal: normal.to_array(), color }
    }

    const ATTRIBUTES: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// One corner of a line quad: both endpoints plus which end / side / kind this corner is.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct LineVertex {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub color: [f32; 4],
    /// x: end (0 = a, 1 = b), y: side (-1 / +1), z: kind (0 grid, 1 wire).
    pub params: [f32; 3],
}

impl LineVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x3];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }

    /// Six corners (two triangles) for the segment `a -> b`.
    pub fn quad(a: &Vertex, b: &Vertex) -> [LineVertex; 6] {
        let kind = if a.normal[1] > 0.5 { 1.0 } else { 0.0 };
        let corner = |end: f32, side: f32| LineVertex { a: a.position, b: b.position, color: a.color, params: [end, side, kind] };
        let (a0, a1, b0, b1) = (corner(0.0, -1.0), corner(0.0, 1.0), corner(1.0, -1.0), corner(1.0, 1.0));
        [a0, b0, b1, a0, b1, a1]
    }
}

/// Expand a line list (vertex pairs) into quad corners.
pub fn expand_lines(lines: &[Vertex]) -> Vec<LineVertex> {
    lines.as_chunks::<2>().0.iter().flat_map(|[a, b]| LineVertex::quad(a, b)).collect()
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    ambient: [f32; 4],
    /// xyz camera position, w grid fade distance.
    camera: [f32; 4],
    /// xy size in pixels, z line width in pixels.
    viewport: [f32; 4],
}

/// What to draw and how.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewOptions {
    pub shading: ShadingMode,
    pub show_grid: bool,
    pub background: [f32; 4],
    /// World direction towards the key light.
    pub light_dir: Vec3,
    /// Width of grid and wire lines in pixels.
    pub line_width_px: f32,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            shading: ShadingMode::Solid,
            show_grid: true,
            // gray-8 (#292e37) from the design tokens, already sRGB-encoded: the clear colour is
            // written to the non-sRGB target as-is
            background: [0x29 as f32 / 255.0, 0x2e as f32 / 255.0, 0x37 as f32 / 255.0, 1.0],
            light_dir: Vec3::new(0.4, 1.0, 0.6),
            line_width_px: 1.5,
        }
    }
}

/// Vertices for one frame, CPU side. Produced by [`build_frame`], consumed by [`Viewport::render`].
#[derive(Clone, Debug, Default)]
pub struct FrameGeometry {
    pub triangles: Vec<Vertex>,
    pub lines: Vec<Vertex>,
}

pub struct Viewport {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    solid: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    tri_buffer: Option<(wgpu::Buffer, u32)>,
    line_buffer: Option<(wgpu::Buffer, u32)>,
    /// Bumped when the colour texture is recreated, so the UI re-registers it.
    pub texture_generation: u64,
}

impl Viewport {
    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    /// `format` is the window's target format; the viewport renders to its non-sRGB twin and
    /// encodes sRGB in the shader, because egui blits native textures without a colour transform.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let format = format.remove_srgb_suffix();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shape viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("viewport.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let make = |label: &str, vs: &str, fs: &str, layout_desc: wgpu::VertexBufferLayout<'static>, cull: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    buffers: &[Some(layout_desc)],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: cull,
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: Self::DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let solid = make("solid", "vs_main", "fs_solid", Vertex::layout(), None);
        let lines = make("lines", "vs_lines", "fs_flat", LineVertex::layout(), None);
        let (color, color_view, depth_view) = Self::make_targets(&device, format, (16, 16));
        Self {
            device,
            queue,
            format,
            size: (16, 16),
            color,
            color_view,
            depth_view,
            globals,
            bind_group,
            solid,
            lines,
            tri_buffer: None,
            line_buffer: None,
            texture_generation: 0,
        }
    }

    fn make_targets(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> (wgpu::Texture, wgpu::TextureView, wgpu::TextureView) {
        let extent = wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport color"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        (color, color_view, depth_view)
    }

    /// Make the targets match the panel. Returns true when the texture was recreated.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        let size = (width.max(1), height.max(1));
        if size == self.size {
            return false;
        }
        let (c, cv, dv) = Self::make_targets(&self.device, self.format, size);
        self.color = c;
        self.color_view = cv;
        self.depth_view = dv;
        self.size = size;
        self.texture_generation += 1;
        true
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }
    pub fn color_view(&self) -> &wgpu::TextureView {
        &self.color_view
    }
    pub fn color_texture(&self) -> &wgpu::Texture {
        &self.color
    }
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Build the frame's geometry from the scene and draw it.
    pub fn render(&mut self, scene: &Scene, camera: &OrbitCamera, options: &ViewOptions) {
        let geometry = build_frame(scene, options);
        self.render_geometry(&geometry, camera, options);
    }

    pub fn render_geometry(&mut self, geometry: &FrameGeometry, camera: &OrbitCamera, options: &ViewOptions) {
        let aspect = self.size.0 as f32 / self.size.1 as f32;
        let g = Globals {
            view_proj: camera.view_projection(aspect).to_cols_array_2d(),
            light_dir: camera_relative_light(camera, options.light_dir).extend(0.0).to_array(),
            ambient: [0.35, 0.37, 0.42, 1.0],
            camera: camera.position().extend((camera.distance * 4.0).max(20.0)).to_array(),
            viewport: [self.size.0 as f32, self.size.1 as f32, options.line_width_px, 0.0],
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));
        let mut tri = self.tri_buffer.take();
        let mut line = self.line_buffer.take();
        self.upload(&mut tri, &geometry.triangles, "triangles");
        let quads = expand_lines(&geometry.lines);
        self.upload(&mut line, &quads, "lines");
        self.tri_buffer = tri;
        self.line_buffer = line;

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport") });
        {
            let bg = options.background;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: bg[3] as f64 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            if let Some((buf, n)) = &self.line_buffer
                && *n > 0
            {
                pass.set_pipeline(&self.lines);
                pass.set_vertex_buffer(0, buf.slice(..u64::from(*n) * std::mem::size_of::<LineVertex>() as u64));
                pass.draw(0..*n, 0..1);
            }
            if let Some((buf, n)) = &self.tri_buffer
                && *n > 0
            {
                pass.set_pipeline(&self.solid);
                pass.set_vertex_buffer(0, buf.slice(..u64::from(*n) * std::mem::size_of::<Vertex>() as u64));
                pass.draw(0..*n, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Upload into a persistent buffer, growing it only when the data no longer fits. Buffers are
    /// never recreated per frame: the GL backend can keep a stale vertex binding when a freshly
    /// created buffer reuses a just-destroyed handle, which drew the grid's line list as triangles.
    fn upload<V: Pod>(&self, slot: &mut Option<(wgpu::Buffer, u32)>, vertices: &[V], label: &str) {
        if vertices.is_empty() {
            if let Some((_, n)) = slot {
                *n = 0;
            }
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(vertices);
        let fits = slot.as_ref().is_some_and(|(b, _)| b.size() >= bytes.len() as u64);
        if !fits {
            let capacity = (bytes.len() as u64).next_power_of_two().max(1 << 16);
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            *slot = Some((buffer, 0));
        }
        let (buffer, n) = slot.as_mut().expect("slot was just filled");
        self.queue.write_buffer(buffer, 0, bytes);
        *n = vertices.len() as u32;
    }
}

/// Key light follows the camera a little so a model never goes fully dark when orbited.
fn camera_relative_light(camera: &OrbitCamera, world_dir: Vec3) -> Vec3 {
    (world_dir.normalize_or_zero() * 0.6 + (-camera.forward() + camera.up() * 0.5).normalize_or_zero() * 0.4).normalize_or_zero()
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "shape-gpu");
    }
}
