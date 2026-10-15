// Shared by the solid and line pipelines.

const MAX_LIGHTS: u32 = 4u;

struct Light {
    // xyz: position (point/spot) or direction towards the light (sun); w: kind 0 point, 1 sun, 2 spot
    position: vec4<f32>,
    // rgb: colour * power; w: unused
    radiance: vec4<f32>,
    // xyz: spot direction (where it shines); w: unused
    direction: vec4<f32>,
    // x: cos(inner), y: cos(outer), zw unused
    cone: vec4<f32>,
};

struct Globals {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    light_dir: vec4<f32>,   // fallback key light, world space, towards the light
    ambient: vec4<f32>,     // rgb ambient, a unused
    camera: vec4<f32>,      // xyz camera position, w = grid fade distance
    viewport: vec4<f32>,    // xy size in pixels, z line width in pixels, w = light count
    shading: vec4<f32>,     // x lighting (0 studio, 1 matcap, 2 flat), y matcap, z cavity, w x-ray alpha
    lights: array<Light, 4>,
};

// Procedural matcaps: a lit sphere seen head-on, indexed by the view-space normal.
fn matcap(index: u32, nv: vec3<f32>) -> vec3<f32> {
    let l = normalize(vec3<f32>(0.35, 0.55, 0.75));
    let ndl = max(dot(nv, l), 0.0);
    let spec = pow(max(dot(nv, normalize(l + vec3<f32>(0.0, 0.0, 1.0))), 0.0), 48.0);
    let rim = pow(1.0 - max(nv.z, 0.0), 3.0);
    if (index == 1u) {
        // Metal: dark base, broad reflection band, sharp highlight.
        let band = smoothstep(-0.2, 0.6, nv.y) * 0.45;
        return vec3<f32>(0.18, 0.19, 0.21) + vec3<f32>(band) + vec3<f32>(spec * 0.9) + vec3<f32>(rim * 0.25);
    } else if (index == 2u) {
        // Red wax: warm, soft falloff, waxy highlight.
        return vec3<f32>(0.62, 0.12, 0.08) * (0.35 + 0.65 * ndl) + vec3<f32>(0.9, 0.6, 0.5) * spec * 0.35 + vec3<f32>(0.4, 0.1, 0.08) * rim;
    } else if (index == 3u) {
        // Ceramic: bright white, glossy.
        return vec3<f32>(0.85, 0.86, 0.88) * (0.45 + 0.55 * ndl) + vec3<f32>(spec * 0.6);
    }
    // Clay: matte grey-beige.
    return vec3<f32>(0.55, 0.52, 0.48) * (0.3 + 0.7 * ndl) + vec3<f32>(spec * 0.08);
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var base_tex: texture_2d<f32>;
@group(1) @binding(1) var base_samp: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    // x metallic, y roughness, z textured flag
    @location(3) material: vec4<f32>,
    @location(4) emissive: vec4<f32>,
    @location(5) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
    @location(3) material: vec4<f32>,
    @location(4) emissive: vec4<f32>,
    @location(5) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.normal = in.normal;
    out.color = in.color;
    out.world = in.position;
    out.material = in.material;
    out.emissive = in.emissive;
    out.uv = in.uv;
    return out;
}

// Lambert + Blinn-Phong from the scene's lights (or a fixed key light when the scene has none),
// a sky/ground hemisphere ambient, metallic tinting the specular and darkening the diffuse.
@fragment
fn fs_solid(in: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let v = normalize(globals.camera.xyz - in.world);
    let metallic = clamp(in.material.x, 0.0, 1.0);
    let roughness = clamp(in.material.y, 0.02, 1.0);
    var base = in.color.rgb;
    if (in.material.z > 0.5) {
        // The texture is sRGB-decoded by the sampler; v = 0 is the bottom row.
        base = base * textureSample(base_tex, base_samp, vec2<f32>(in.uv.x, 1.0 - in.uv.y)).rgb;
    }
    // Cavity (approximation): the normal's screen-space rate of change darkens creases and
    // edges, where Blender's screen-space cavity shows its valleys and ridges.
    var cavity = 1.0;
    if (globals.shading.z > 0.5) {
        let curvature = length(fwidth(n));
        cavity = clamp(1.0 - curvature * 1.5, 0.35, 1.0);
    }
    let lighting = u32(globals.shading.x);
    let alpha = globals.shading.w;
    if (lighting == 2u) {
        // Flat: the colour as it is, no lighting.
        return vec4<f32>(to_srgb(base * cavity), alpha);
    }
    if (lighting == 1u) {
        let nv = normalize((globals.view * vec4<f32>(n, 0.0)).xyz);
        let lit = matcap(u32(globals.shading.y), nv);
        return vec4<f32>(to_srgb(lit * cavity), alpha);
    }
    let diffuse_color = base * (1.0 - metallic * 0.85);
    let spec_color = mix(vec3<f32>(0.04), base, metallic);
    let shininess = mix(256.0, 4.0, roughness);
    let hemi = 0.5 + 0.5 * n.y;
    var color = diffuse_color * globals.ambient.rgb * hemi;

    let count = u32(globals.viewport.w);
    if (count == 0u) {
        let l = normalize(globals.light_dir.xyz);
        let ndl = max(dot(n, l), 0.0);
        let h = normalize(l + v);
        let spec = pow(max(dot(n, h), 0.0), shininess) * (1.0 - roughness) * 0.5;
        color = color + diffuse_color * ndl * 0.8 + spec_color * spec;
    } else {
        for (var i = 0u; i < count && i < MAX_LIGHTS; i = i + 1u) {
            let light = globals.lights[i];
            var l: vec3<f32>;
            var radiance: vec3<f32>;
            if (light.position.w == 1.0) {
                l = normalize(light.position.xyz);
                radiance = light.radiance.rgb;
            } else {
                let to_light = light.position.xyz - in.world;
                let d2 = max(dot(to_light, to_light), 0.01);
                l = to_light * inverseSqrt(d2);
                radiance = light.radiance.rgb / d2;
                if (light.position.w == 2.0) {
                    let cos_angle = dot(-l, normalize(light.direction.xyz));
                    radiance = radiance * smoothstep(light.cone.y, light.cone.x, cos_angle);
                }
            }
            let ndl = max(dot(n, l), 0.0);
            let h = normalize(l + v);
            let spec = pow(max(dot(n, h), 0.0), shininess) * (1.0 - roughness) * 0.5;
            color = color + (diffuse_color * ndl + spec_color * spec) * radiance;
        }
    }
    color = color + in.emissive.rgb;
    // soft roll-off instead of a hard clip
    color = color / (color + vec3<f32>(0.6)) * 1.6;
    return vec4<f32>(to_srgb(clamp(color * cavity, vec3<f32>(0.0), vec3<f32>(1.0))), in.color.a * alpha);
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
    var out: VertexOut;
    out.color = in.color;
    out.normal = vec3<f32>(0.0, in.params.z, 0.0);
    out.material = vec4<f32>(0.0);
    out.emissive = vec4<f32>(0.0);
    // Clip against w = eps so no endpoint sits behind the eye.
    if (ca.w < eps && cb.w < eps) {
        out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0); // outside the depth range: discarded
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
    if (in.params.z > 1.5) {
        // A point: a square of 2 * (2 * line width) pixels around a.
        let half = globals.viewport.z * 2.0;
        let px = vec2<f32>(select(-1.0, 1.0, in.params.x > 0.5), in.params.y) * half;
        clip = vec4<f32>(ca.xy + px / (size * 0.5) * ca.w, ca.z - 0.0008 * ca.w, ca.w);
        out.clip = clip;
        out.world = in.a;
        out.normal = vec3<f32>(0.0, 1.0, 0.0);
        return out;
    }
    clip = vec4<f32>(clip.xy + offset_ndc * clip.w, clip.z, clip.w);
    // Nudge towards the camera so wire overlays win the depth test against their faces.
    clip.z = clip.z - 0.0004 * clip.w;
    out.clip = clip;
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
