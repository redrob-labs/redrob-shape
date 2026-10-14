// Shared by the solid and line pipelines.

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,   // world space, towards the light
    ambient: vec4<f32>,     // rgb ambient, a unused
    camera: vec4<f32>,      // xyz camera position, w = grid fade distance
    viewport: vec4<f32>,    // xy size in pixels, z line width in pixels, w unused
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.normal = in.normal;
    out.color = in.color;
    out.world = in.position;
    return out;
}

// Flat Lambert with a sky/ground hemisphere term, no specular yet.
@fragment
fn fs_solid(in: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let diffuse = max(dot(n, normalize(globals.light_dir.xyz)), 0.0);
    let hemi = 0.5 + 0.5 * n.y;
    let lit = in.color.rgb * (globals.ambient.rgb * hemi + diffuse * 0.8);
    return vec4<f32>(to_srgb(lit), in.color.a);
}

// ---- lines ----
//
// Lines are drawn as screen-space quads (two triangles per segment), not as a native line list:
// native lines are 1 px only on Vulkan/Metal and the software Vulkan rasterizer (llvmpipe) turned
// segments that cross behind the camera into filled slabs. Each vertex carries both endpoints; the
// shader clips the segment to the near plane in clip space, then pushes the vertex sideways by half
// the line width in pixels.

struct LineIn {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) color: vec4<f32>,
    // x: which end this vertex belongs to (0 = a, 1 = b); y: side (-1 or +1); z: kind (0 grid, 1 wire)
    @location(3) params: vec3<f32>,
};

@vertex
fn vs_lines(in: LineIn) -> VertexOut {
    var ca = globals.view_proj * vec4<f32>(in.a, 1.0);
    var cb = globals.view_proj * vec4<f32>(in.b, 1.0);
    let eps = 0.001;
    // Clip against w = eps so no endpoint sits behind the eye.
    if (ca.w < eps && cb.w < eps) {
        var out: VertexOut;
        out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0); // outside the depth range: discarded
        out.color = in.color;
        out.normal = vec3<f32>(0.0, in.params.z, 0.0);
        out.world = in.a;
        return out;
    }
    if (ca.w < eps) {
        let t = (eps - ca.w) / (cb.w - ca.w);
        ca = mix(ca, cb, t);
    } else if (cb.w < eps) {
        let t = (eps - cb.w) / (ca.w - cb.w);
        cb = mix(cb, ca, t);
    }
    let na = ca.xy / ca.w;
    let nb = cb.xy / cb.w;
    let size = globals.viewport.xy;
    var dir = (nb - na) * size * 0.5;        // pixels
    let len = length(dir);
    if (len > 0.0) { dir = dir / len; } else { dir = vec2<f32>(1.0, 0.0); }
    let normal_px = vec2<f32>(-dir.y, dir.x) * (globals.viewport.z * 0.5) * in.params.y;
    let offset_ndc = normal_px / (size * 0.5);

    var clip = select(ca, cb, in.params.x > 0.5);
    clip = vec4<f32>(clip.xy + offset_ndc * clip.w, clip.z, clip.w);
    // Nudge towards the camera so wire overlays win the depth test against their faces.
    clip.z = clip.z - 0.0004 * clip.w;

    var out: VertexOut;
    out.clip = clip;
    out.color = in.color;
    out.normal = vec3<f32>(0.0, in.params.z, 0.0);
    out.world = select(in.a, in.b, in.params.x > 0.5);
    return out;
}

// Lines carry their colour. Grid lines (kind 0) fade with distance so the far grid does not
// collapse into a solid band where its lines pack tighter than a pixel.
@fragment
fn fs_flat(in: VertexOut) -> @location(0) vec4<f32> {
    var alpha = in.color.a;
    if (in.normal.y < 0.5) {
        let d = distance(in.world, globals.camera.xyz);
        alpha = alpha * clamp(1.0 - d / globals.camera.w, 0.0, 1.0);
    }
    return vec4<f32>(to_srgb(in.color.rgb), alpha);
}

// The colour target is a non-sRGB texture that egui blits as-is, so encode here.
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c < vec3<f32>(0.0031308);
    let lower = c * 12.92;
    let higher = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(higher, lower, cutoff);
}
