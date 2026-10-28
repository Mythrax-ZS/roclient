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
    params: vec4<f32>,
    // The sun's view of the sprite shadow casters.
    light_vp: mat4x4<f32>,
    // x: strength (0 off), y: shadow-map texel size, z: softness in texels
    shadow: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_samp: sampler_comparison;
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
    @location(4) world: vec3<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.pos = g.view_proj * vec4(v.pos, 1.0);
    o.normal = v.normal;
    o.uv = v.uv;
    o.lightmap_uv = v.lightmap_uv;
    o.color = v.color;
    o.world = v.pos;
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
    return vec4(srgb_to_linear(rgb) * sprite_shadow(in.world), 1.0);
}

// How lit a point is by the sun with only sprites as casters: 1 lit, 1 - strength fully
// shadowed. A 5x5 PCF kernel (scaled by softness) gives soft, painterly edges.
fn sprite_shadow(world: vec3<f32>) -> f32 {
    if g.shadow.x <= 0.0 {
        return 1.0;
    }
    let lp = g.light_vp * vec4(world, 1.0);
    let ndc = lp.xyz / lp.w;
    let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) || ndc.z > 1.0 {
        return 1.0;
    }
    let step = g.shadow.y * max(g.shadow.z, 0.5);
    var lit = 0.0;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            lit += textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2(f32(x), f32(y)) * step, ndc.z - 0.0015);
        }
    }
    lit /= 25.0;
    return 1.0 - g.shadow.x * (1.0 - lit);
}
