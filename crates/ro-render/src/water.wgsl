// Animated water surface with sine waves, alpha-blended over the ground.

struct Globals {
    view_proj: mat4x4<f32>,
    focus: vec4<f32>,
    eye: vec4<f32>,
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    // x: seconds since start
    params: vec4<f32>,
};

struct Water {
    // level (y-up), wave height, wave speed (deg/tick), wave pitch (deg/tile)
    wave: vec4<f32>,
    anim: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var<uniform> w: Water;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) tile: vec2<f32>) -> VsOut {
    let ticks = g.params.x * 60.0;
    let phase = radians(ticks * w.wave.z + (tile.x + tile.y) * w.wave.w * 0.5);
    let y = w.wave.x + sin(phase) * w.wave.y;
    var o: VsOut;
    o.pos = g.view_proj * vec4(pos.x, y, pos.y, 1.0);
    // One texture repeat every 4 tiles.
    o.uv = tile / 4.0;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    return vec4(srgb_to_linear(t.rgb), 0.56);
}
