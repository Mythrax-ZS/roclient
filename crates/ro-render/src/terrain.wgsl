// GND terrain: textured, vertex-tinted, alpha-tested (RO magenta cutouts).

struct Globals {
    view_proj: mat4x4<f32>,
    focus: vec4<f32>,
    eye: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec3<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.pos = g.view_proj * vec4(v.pos, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    if t.a < 0.5 {
        discard;
    }
    return vec4(t.rgb * in.color, 1.0);
}
