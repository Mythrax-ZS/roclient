// Sprite shadow casters: each sprite layer as an upright card facing the light, seen from
// the sun. Texels below the alpha cut are discarded so the shadow keeps the silhouette.

struct Shadow {
    light_vp: mat4x4<f32>,
    // xyz: camera right (horizontal), w: world units per sprite pixel
    right: vec4<f32>,
    // xyz: a torch's position, w: 1 to turn each card toward it
    light: vec4<f32>,
};

@group(0) @binding(0) var<uniform> s: Shadow;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsIn {
    @location(0) anchor: vec3<f32>,
    @location(1) offset: vec2<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    // Sprite pixels: x right, y down. World: up is +y.
    let away = v.anchor - s.light.xyz;
    let right = select(s.right.xyz, normalize(vec3(-away.z, 0.0, away.x) + vec3(1e-4, 0.0, 0.0)), s.light.w > 0.5);
    let world = v.anchor + right * v.offset.x * s.right.w + vec3(0.0, -v.offset.y * s.right.w, 0.0);
    var o: VsOut;
    o.pos = s.light_vp * vec4(world, 1.0);
    o.uv = v.uv;
    o.alpha = v.color.a;
    return o;
}

@fragment
fn fs_main(in: VsOut) {
    if textureSample(tex, samp, in.uv).a * in.alpha < 0.5 {
        discard;
    }
}
