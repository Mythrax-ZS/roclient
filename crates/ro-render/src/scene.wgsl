// World geometry (ground + models). Lighting is computed in gamma space like the
// original client, then converted for the sRGB target.

struct Globals {
    view_proj: mat4x4<f32>,
    focus: vec4<f32>,
    eye: vec4<f32>,
    // xyz: direction the sun travels (y-up world)
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var lightmap: texture_2d<f32>;
@group(1) @binding(3) var lightmap_samp: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) lightmap_uv: vec2<f32>,
    @location(4) color: vec3<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) lightmap_uv: vec2<f32>,
    @location(3) color: vec3<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.pos = g.view_proj * vec4(v.pos, 1.0);
    o.normal = v.normal;
    o.uv = v.uv;
    o.lightmap_uv = v.lightmap_uv;
    o.color = v.color;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    if t.a < 0.5 {
        discard;
    }
    let lm = textureSample(lightmap, lightmap_samp, in.lightmap_uv);
    let ndl = max(dot(normalize(in.normal), -g.light_dir.xyz), 0.0);
    let light = clamp(g.ambient.rgb + g.diffuse.rgb * ndl, vec3(0.0), vec3(1.0)) * lm.a;
    let rgb = clamp(t.rgb * in.color * light + lm.rgb, vec3(0.0), vec3(1.0));
    return vec4(srgb_to_linear(rgb), 1.0);
}
