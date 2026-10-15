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

pub use camera::{AxisView, OrbitCamera};
pub use geometry::{PickHit, ShadingMode, build_frame, display_mesh, nearest_edge, nearest_vertex, pick, pick_detail, scene_lights, weight_color};

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
    /// x metallic, y roughness, z 1 when the base colour is multiplied by the bound texture.
    pub material: [f32; 4],
    pub emissive: [f32; 4],
    pub uv: [f32; 2],
}

impl Vertex {
    pub fn new(position: Vec3, normal: Vec3, color: [f32; 4]) -> Self {
        Self { position: position.to_array(), normal: normal.to_array(), color, material: [0.0, 0.5, 0.0, 0.0], emissive: [0.0; 4], uv: [0.0; 2] }
    }

    pub fn with_uv(mut self, uv: shape_math::Vec2) -> Self {
        self.uv = uv.to_array();
        self.material[2] = 1.0;
        self
    }

    pub fn with_material(mut self, metallic: f32, roughness: f32, emissive: [f32; 3]) -> Self {
        self.material = [metallic, roughness, 0.0, 0.0];
        self.emissive = [emissive[0], emissive[1], emissive[2], 0.0];
        self
    }

    const ATTRIBUTES: [wgpu::VertexAttribute; 6] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x2];

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

impl LineVertex {
    /// Six corners of a screen-space square around a point (kind 2; the shader sizes it).
    pub fn point(p: &Vertex) -> [LineVertex; 6] {
        let corner = |end: f32, side: f32| LineVertex { a: p.position, b: p.position, color: p.color, params: [end, side, 2.0] };
        let (a0, a1, b0, b1) = (corner(0.0, -1.0), corner(0.0, 1.0), corner(1.0, -1.0), corner(1.0, 1.0));
        [a0, b0, b1, a0, b1, a1]
    }
}

/// Expand a line list (vertex pairs) into quad corners.
pub fn expand_lines(lines: &[Vertex]) -> Vec<LineVertex> {
    lines.as_chunks::<2>().0.iter().flat_map(|[a, b]| LineVertex::quad(a, b)).collect()
}

/// Expand points into screen-space squares.
pub fn expand_points(points: &[Vertex]) -> Vec<LineVertex> {
    points.iter().flat_map(LineVertex::point).collect()
}

/// One scene light as the shader sees it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct GpuLight {
    /// xyz position (point/spot) or direction towards the light (sun); w kind: 0 point, 1 sun, 2 spot.
    pub position: [f32; 4],
    /// rgb colour * power.
    pub radiance: [f32; 4],
    /// xyz direction a spot shines along.
    pub direction: [f32; 4],
    /// x cos(inner), y cos(outer).
    pub cone: [f32; 4],
}

pub const MAX_LIGHTS: usize = 4;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    light_dir: [f32; 4],
    ambient: [f32; 4],
    /// xyz camera position, w grid fade distance.
    camera: [f32; 4],
    /// xy size in pixels, z line width in pixels, w light count.
    viewport: [f32; 4],
    /// x lighting (0 studio, 1 matcap, 2 flat), y matcap index, z cavity strength, w x-ray alpha.
    shading: [f32; 4],
    lights: [GpuLight; MAX_LIGHTS],
}

/// Blender's Solid-mode lighting choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lighting {
    #[default]
    Studio,
    MatCap,
    Flat,
}

/// Blender's Solid-mode colour source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// The material's viewport display colour.
    #[default]
    Material,
    /// One colour for every object.
    Object,
    /// A stable random hue per object.
    Random,
    /// `SolidOptions::single_color`.
    Single,
}

/// Blender's viewport shading popover for Solid mode.
#[derive(Clone, Debug, PartialEq)]
pub struct SolidOptions {
    pub lighting: Lighting,
    pub matcap: u32,
    pub color: ColorMode,
    pub single_color: [f32; 4],
    pub backface_culling: bool,
    pub xray: bool,
    pub xray_alpha: f32,
    pub cavity: bool,
}

impl Default for SolidOptions {
    fn default() -> Self {
        Self {
            lighting: Lighting::Studio,
            matcap: 0,
            color: ColorMode::Material,
            single_color: [0.8, 0.8, 0.8, 1.0],
            backface_culling: false,
            xray: false,
            xray_alpha: 0.5,
            cavity: false,
        }
    }
}

/// The matcaps the shader can draw (procedural spheres, see `viewport.wgsl`).
pub const MATCAPS: [&str; 4] = ["Clay", "Metal", "Red Wax", "Ceramic"];

/// What to draw and how.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewOptions {
    pub shading: ShadingMode,
    pub solid: SolidOptions,
    /// Weight Paint: colour this object's vertices by the named vertex group's weight.
    pub weight_paint: Option<(shape_scene::ObjectId, String)>,
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
            solid: SolidOptions::default(),
            weight_paint: None,
            show_grid: true,
            // Blender's default 3D viewport background (#3f3f3f), already sRGB-encoded: the clear
            // colour is written to the non-sRGB target as-is
            background: [0x3f as f32 / 255.0, 0x3f as f32 / 255.0, 0x3f as f32 / 255.0, 1.0],
            light_dir: Vec3::new(0.4, 1.0, 0.6),
            line_width_px: 1.5,
        }
    }
}

/// Vertices for one frame, CPU side. Produced by [`build_frame`], consumed by [`Viewport::render`].
#[derive(Clone, Debug, Default)]
pub struct FrameGeometry {
    pub triangles: Vec<Vertex>,
    /// Triangles drawn with an image texture, grouped by texture.
    pub textured: Vec<(shape_scene::TextureId, Vec<Vertex>)>,
    pub lines: Vec<Vertex>,
    /// Edit-mode vertex dots, drawn as screen-space squares after the lines.
    pub points: Vec<Vertex>,
    /// Scene lights, at most [`MAX_LIGHTS`] are uploaded.
    pub lights: Vec<GpuLight>,
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
    solid_cull: wgpu::RenderPipeline,
    solid_xray: wgpu::RenderPipeline,
    solid_cull_xray: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    tri_buffer: Option<(wgpu::Buffer, u32)>,
    /// One vertex buffer per textured group, reused frame to frame.
    textured_buffers: Vec<Option<(wgpu::Buffer, u32)>>,
    texture_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// 1x1 white, bound for untextured draws.
    white: wgpu::BindGroup,
    /// Uploaded textures by id, with the (width, height, path) they were built from.
    textures: std::collections::HashMap<shape_scene::TextureId, (wgpu::BindGroup, (u32, u32, String))>,
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
        let texture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = Self::upload_texture(&device, &queue, &texture_bgl, &sampler, 1, 1, &[255, 255, 255, 255]);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport"),
            bind_group_layouts: &[Some(&bgl), Some(&texture_bgl)],
            immediate_size: 0,
        });
        let make = |label: &str, vs: &str, fs: &str, layout_desc: wgpu::VertexBufferLayout<'static>, cull: Option<wgpu::Face>, depth_write: bool| {
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
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let solid = make("solid", "vs_main", "fs_solid", Vertex::layout(), None, true);
        let solid_cull = make("solid-cull", "vs_main", "fs_solid", Vertex::layout(), Some(wgpu::Face::Back), true);
        // X-ray: see-through surfaces that do not occlude each other.
        let solid_xray = make("solid-xray", "vs_main", "fs_solid", Vertex::layout(), None, false);
        let solid_cull_xray = make("solid-cull-xray", "vs_main", "fs_solid", Vertex::layout(), Some(wgpu::Face::Back), false);
        let lines = make("lines", "vs_lines", "fs_flat", LineVertex::layout(), None, true);
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
            solid_cull,
            solid_xray,
            solid_cull_xray,
            lines,
            tri_buffer: None,
            textured_buffers: Vec::new(),
            texture_bgl,
            sampler,
            white,
            textures: std::collections::HashMap::new(),
            line_buffer: None,
            texture_generation: 0,
        }
    }

    /// Upload sRGB8 pixels as a sampled texture and bind it with the shared sampler.
    fn upload_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bgl: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) -> wgpu::BindGroup {
        let size = wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w.max(1)), rows_per_image: Some(h.max(1)) },
            size,
        );
        let view = texture.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image texture"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    /// Make sure every texture the frame uses is on the GPU (re-uploading one whose pixels changed).
    fn sync_textures(&mut self, scene: &Scene, geometry: &FrameGeometry) {
        for (id, _) in &geometry.textured {
            let Some(t) = scene.texture(*id) else { continue };
            let key = (t.width, t.height, t.path.clone());
            if self.textures.get(id).is_some_and(|(_, k)| *k == key) {
                continue;
            }
            let (w, h, pixels) = if t.is_loaded() { (t.width, t.height, t.to_srgb8()) } else { (1, 1, vec![255, 0, 255, 255]) };
            let bg = Self::upload_texture(&self.device, &self.queue, &self.texture_bgl, &self.sampler, w, h, &pixels);
            self.textures.insert(*id, (bg, key));
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
        self.sync_textures(scene, &geometry);
        self.render_geometry(&geometry, camera, options);
    }

    pub fn render_geometry(&mut self, geometry: &FrameGeometry, camera: &OrbitCamera, options: &ViewOptions) {
        let aspect = self.size.0 as f32 / self.size.1 as f32;
        let g = Globals {
            view_proj: camera.view_projection(aspect).to_cols_array_2d(),
            view: camera.view().to_cols_array_2d(),
            light_dir: camera_relative_light(camera, options.light_dir).extend(0.0).to_array(),
            ambient: [0.35, 0.37, 0.42, 1.0],
            camera: camera.position().extend((camera.distance * 4.0).max(20.0)).to_array(),
            viewport: [self.size.0 as f32, self.size.1 as f32, options.line_width_px, geometry.lights.len().min(MAX_LIGHTS) as f32],
            shading: {
                let s = &options.solid;
                let solid = options.shading != ShadingMode::MaterialPreview;
                let lighting = if !solid { 0.0 } else { s.lighting as u32 as f32 };
                [lighting, s.matcap as f32, if solid && s.cavity { 1.0 } else { 0.0 }, if s.xray { s.xray_alpha } else { 1.0 }]
            },
            lights: {
                let mut arr = [GpuLight::default(); MAX_LIGHTS];
                for (slot, l) in arr.iter_mut().zip(&geometry.lights) {
                    *slot = *l;
                }
                arr
            },
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));
        let mut tri = self.tri_buffer.take();
        let mut line = self.line_buffer.take();
        self.upload(&mut tri, &geometry.triangles, "triangles");
        self.textured_buffers.resize_with(geometry.textured.len(), || None);
        let mut textured = std::mem::take(&mut self.textured_buffers);
        for (slot, (_, verts)) in textured.iter_mut().zip(&geometry.textured) {
            self.upload(slot, verts, "textured triangles");
        }
        self.textured_buffers = textured;
        let mut quads = expand_lines(&geometry.lines);
        quads.extend(expand_points(&geometry.points));
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
            pass.set_bind_group(1, &self.white, &[]);
            let solid = match (options.solid.backface_culling, options.solid.xray) {
                (false, false) => &self.solid,
                (true, false) => &self.solid_cull,
                (false, true) => &self.solid_xray,
                (true, true) => &self.solid_cull_xray,
            };
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
                pass.set_pipeline(solid);
                pass.set_vertex_buffer(0, buf.slice(..u64::from(*n) * std::mem::size_of::<Vertex>() as u64));
                pass.draw(0..*n, 0..1);
            }
            for (slot, (id, _)) in self.textured_buffers.iter().zip(&geometry.textured) {
                let Some((buf, n)) = slot else { continue };
                if *n == 0 {
                    continue;
                }
                let bg = self.textures.get(id).map(|(bg, _)| bg).unwrap_or(&self.white);
                pass.set_pipeline(solid);
                pass.set_bind_group(1, bg, &[]);
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
