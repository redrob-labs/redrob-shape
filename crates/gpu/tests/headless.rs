//! Render a frame on whatever adapter the machine has and read the pixels back. Skips (passes)
//! when there is no adapter at all, so the test suite still runs on a bare container.

use shape_geom::primitives;
use shape_gpu::{OrbitCamera, ViewOptions, Viewport};
use shape_math::Vec3;
use shape_scene::{EditSelection, Scene, SelectMode};

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

/// RGBA8 pixels of a rendered frame.
fn render(scene: &Scene, camera: &OrbitCamera, options: &ViewOptions, w: u32, h: u32) -> Option<Vec<[u8; 4]>> {
    let (device, queue) = device()?;
    let mut vp = Viewport::new(device.clone(), queue.clone(), wgpu::TextureFormat::Rgba8Unorm);
    vp.resize(w, h);
    vp.render(scene, camera, options);
    let bytes_per_row = (w * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(bytes_per_row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        vp.color_texture().as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    let data = slice.get_mapped_range().ok()?;
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        let row = &data[(y * bytes_per_row) as usize..];
        for x in 0..w {
            let i = (x * 4) as usize;
            out.push([row[i], row[i + 1], row[i + 2], row[i + 3]]);
        }
    }
    Some(out)
}

#[test]
fn vertex_mode_draws_dots_and_selection_orange() {
    let mut scene = Scene::new("t");
    let id = scene.add_mesh_object("Cube", primitives::cube(2.0), None);
    let mesh = scene.object_mesh(id).unwrap().1.clone();
    let mut e = EditSelection::new(id);
    e.mode = SelectMode::Vertex;
    e.verts = mesh.vertex_ids().filter(|&v| mesh.position(v).y > 0.0).collect();
    e.flush(&mesh);
    scene.edit = Some(e);
    let cam = OrbitCamera { yaw: 0.6, pitch: 0.5, distance: 6.0, ..Default::default() };
    let options = ViewOptions { show_grid: false, ..Default::default() };
    let Some(px) = render(&scene, &cam, &options, 320, 240) else {
        eprintln!("no GPU adapter: skipping");
        return;
    };
    let black = px.iter().filter(|p| p[0] < 10 && p[1] < 10 && p[2] < 10).count();
    let orange = px.iter().filter(|p| p[0] > 200 && p[1] > 110 && p[1] < 180 && p[2] < 90).count();
    let white = px.iter().filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200).count();
    eprintln!("black {black} orange {orange} white {white}");
    assert!(white > 1000, "the cube is drawn");
    assert!(black > 20, "unselected vertex dots are black squares");
    assert!(orange > 20, "selected vertices and edges are orange");
    // Object mode: no dots, no orange.
    scene.edit = None;
    let px = render(&scene, &cam, &options, 320, 240).unwrap();
    let black = px.iter().filter(|p| p[0] < 10 && p[1] < 10 && p[2] < 10).count();
    assert!(black < 5, "no dots outside vertex mode: {black}");
    let _ = Vec3::ZERO;
}

#[test]
fn material_preview_shows_the_texture_and_solid_hides_materials() {
    let mut scene = Scene::new("t");
    // Pure red texture on a white material; Solid must ignore it, Material Preview must show it.
    let tex = scene.add_texture(shape_scene::Texture::from_rgba("red", 2, 2, vec![[1.0, 0.0, 0.0, 1.0]; 4]));
    let mat = scene.add_material(shape_scene::Material { base_color: [1.0, 1.0, 1.0, 1.0], base_color_texture: Some(tex), ..Default::default() });
    let id = scene.add_mesh_object("Cube", primitives::cube(2.0), Some(mat));
    let (mesh_id, mesh) = scene.object_mesh(id).unwrap();
    let uvs = shape_ops::uv_box(mesh);
    scene.mesh_mut(mesh_id).unwrap().uvs = Some(uvs);
    let cam = OrbitCamera { yaw: 0.6, pitch: 0.5, distance: 6.0, ..Default::default() };
    let reddish = |px: &[[u8; 4]]| px.iter().filter(|p| p[0] > 120 && p[1] < 60 && p[2] < 60).count();
    let solid = ViewOptions { show_grid: false, shading: shape_gpu::ShadingMode::Solid, ..Default::default() };
    let Some(px) = render(&scene, &cam, &solid, 320, 240) else {
        eprintln!("no GPU adapter: skipping");
        return;
    };
    assert_eq!(reddish(&px), 0, "Solid shading is studio grey");
    let preview = ViewOptions { show_grid: false, shading: shape_gpu::ShadingMode::MaterialPreview, ..Default::default() };
    let px = render(&scene, &cam, &preview, 320, 240).unwrap();
    assert!(reddish(&px) > 1000, "Material Preview samples the texture: {} red px", reddish(&px));
}

#[test]
fn material_preview_colours_untextured_materials() {
    let mut scene = Scene::new("t");
    let gold = scene.add_material(shape_scene::Material { base_color: [1.0, 0.2, 0.2, 1.0], metallic: 0.0, roughness: 0.5, ..Default::default() });
    scene.add_mesh_object("Sphere", primitives::uv_sphere(1.0, 32, 16), Some(gold));
    scene.add_object(shape_scene::Object::new("Sun", shape_scene::ObjectData::Light(shape_scene::Light::sun(3.0))));
    let cam = OrbitCamera { yaw: 0.0, pitch: 0.3, distance: 5.0, ..Default::default() };
    let preview = ViewOptions { show_grid: false, shading: shape_gpu::ShadingMode::MaterialPreview, ..Default::default() };
    let Some(px) = render(&scene, &cam, &preview, 320, 240) else { return };
    let reddish = px.iter().filter(|p| p[0] > p[1] + 40 && p[0] > p[2] + 40).count();
    assert!(reddish > 500, "material colour shows under the sun: {reddish}");
}

#[test]
fn solid_shading_popover_options_change_the_pixels() {
    use shape_gpu::{ColorMode, Lighting, SolidOptions};
    let mut scene = Scene::new("t");
    let m = scene.add_material(shape_scene::Material { viewport_color: [0.8, 0.8, 0.8, 1.0], ..Default::default() });
    scene.add_mesh_object("Sphere", primitives::uv_sphere(1.0, 32, 16), Some(m));
    let cam = OrbitCamera { yaw: 0.0, pitch: 0.3, distance: 4.0, ..Default::default() };
    let opts = |solid: SolidOptions| ViewOptions { show_grid: false, solid, ..Default::default() };
    let Some(studio) = render(&scene, &cam, &opts(SolidOptions::default()), 160, 120) else { return };
    let distinct = |px: &[[u8; 4]]| px.iter().map(|p| (p[0] / 8, p[1] / 8, p[2] / 8)).collect::<std::collections::HashSet<_>>().len();
    // Flat: the sphere is one colour (no shading), so far fewer distinct shades than Studio.
    let flat = render(&scene, &cam, &opts(SolidOptions { lighting: Lighting::Flat, ..Default::default() }), 160, 120).unwrap();
    assert!(distinct(&flat) < distinct(&studio) / 2, "flat {} studio {}", distinct(&flat), distinct(&studio));
    // Red wax matcap: red pixels where Studio has none.
    let wax = render(&scene, &cam, &opts(SolidOptions { lighting: Lighting::MatCap, matcap: 2, ..Default::default() }), 160, 120).unwrap();
    let red = |px: &[[u8; 4]]| px.iter().filter(|p| p[0] as i32 > p[1] as i32 + 60 && p[0] as i32 > p[2] as i32 + 60).count();
    assert!(red(&wax) > 500 && red(&studio) == 0, "wax {} studio {}", red(&wax), red(&studio));
    // Single colour: the chosen blue shows.
    let single =
        render(&scene, &cam, &opts(SolidOptions { color: ColorMode::Single, single_color: [0.1, 0.2, 0.9, 1.0], ..Default::default() }), 160, 120)
            .unwrap();
    assert!(single.iter().filter(|p| p[2] as i32 > p[0] as i32 + 60).count() > 500);
    // X-ray: the sphere lets the background through, so the centre pixel is darker than opaque.
    let xray = render(&scene, &cam, &opts(SolidOptions { xray: true, ..Default::default() }), 160, 120).unwrap();
    let c = 60 * 160 + 80;
    assert!(xray[c][0] < studio[c][0], "x-ray {:?} opaque {:?}", xray[c], studio[c]);
}
