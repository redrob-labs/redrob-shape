// Shared by the solid and line pipelines.

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,   // world space, towards the light
    ambient: vec4<f32>,     // rgb ambient, a unused
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
};

fn transform(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.normal = in.normal;
    out.color = in.color;
    return out;
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    return transform(in);
}

// Lines: same transform, nudged towards the camera so wire overlays win the depth test
// against the faces they sit on (depth bias is not allowed on line topology).
@vertex
fn vs_lines(in: VertexIn) -> VertexOut {
    var out = transform(in);
    out.clip.z = out.clip.z - 0.0004 * out.clip.w;
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

// Lines carry their colour; normals are ignored.
@fragment
fn fs_flat(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(to_srgb(in.color.rgb), in.color.a);
}

// The colour target is a non-sRGB texture that egui blits as-is, so encode here.
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c < vec3<f32>(0.0031308);
    let lower = c * 12.92;
    let higher = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(higher, lower, cutoff);
}
