// HD-2D post-processing: bloom (bright pass + blur), tilt-shift depth of field, tone
// shoulder, colour grading and vignette. Every pass is a fullscreen triangle.

struct Params {
    // xy: 1 / source size in texels, zw: blur direction (texels)
    texel: vec4<f32>,
    // x: threshold, y: soft knee, z: intensity, w: unused
    bloom: vec4<f32>,
    // x: strength 0..1, y: focus height (0 top .. 1 bottom), z: sharp band half-height, w: falloff
    dof: vec4<f32>,
    // x: exposure, y: contrast, z: saturation, w: warmth (-1 cool .. 1 warm)
    grade: vec4<f32>,
    // x: vignette strength, y: aspect ratio, z: shadow lift, w: unused
    misc: vec4<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> p: Params;
@group(0) @binding(3) var blurred: texture_2d<f32>;
@group(0) @binding(4) var bloom_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // One triangle covering the screen.
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4(xy * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2(xy.x, 1.0 - xy.y);
    return o;
}

// 4-tap box downsample (bilinear taps cover a 4x4 area).
fn down4(uv: vec2<f32>) -> vec3<f32> {
    let o = p.texel.xy;
    return (textureSample(src, samp, uv + vec2(-o.x, -o.y)).rgb
          + textureSample(src, samp, uv + vec2( o.x, -o.y)).rgb
          + textureSample(src, samp, uv + vec2(-o.x,  o.y)).rgb
          + textureSample(src, samp, uv + vec2( o.x,  o.y)).rgb) * 0.25;
}

@fragment
fn fs_down(in: VsOut) -> @location(0) vec4<f32> {
    return vec4(down4(in.uv), 1.0);
}

// Downsample keeping only what is brighter than the threshold (soft knee).
@fragment
fn fs_bright(in: VsOut) -> @location(0) vec4<f32> {
    let c = down4(in.uv);
    let l = max(c.r, max(c.g, c.b));
    let knee = p.bloom.x * p.bloom.y + 1e-4;
    let soft = clamp(l - p.bloom.x + knee, 0.0, 2.0 * knee);
    let w = max(soft * soft / (4.0 * knee), l - p.bloom.x) / max(l, 1e-4);
    return vec4(c * w, 1.0);
}

// 9-tap Gaussian along p.texel.zw.
@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4<f32> {
    let d = p.texel.zw;
    var c = textureSample(src, samp, in.uv).rgb * 0.2270270;
    c += (textureSample(src, samp, in.uv + d * 1.3846153).rgb + textureSample(src, samp, in.uv - d * 1.3846153).rgb) * 0.3162162;
    c += (textureSample(src, samp, in.uv + d * 3.2307692).rgb + textureSample(src, samp, in.uv - d * 3.2307692).rgb) * 0.0702702;
    return vec4(c, 1.0);
}

// Identity below 0.8, then rolls off toward 1 so bloom and bright effects don't clip.
fn shoulder(c: vec3<f32>) -> vec3<f32> {
    let k = 0.8;
    let over = max(c - vec3(k), vec3(0.0));
    return min(c, vec3(k)) + (1.0 - k) * (1.0 - exp(-over / (1.0 - k)));
}

@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    let sharp = textureSample(src, samp, in.uv).rgb;
    // Tilt-shift: sharp in a band around the player, blurring toward top and bottom.
    let d = abs(in.uv.y - p.dof.y);
    let t = smoothstep(p.dof.z, p.dof.z + p.dof.w, d) * p.dof.x;
    var c = mix(sharp, textureSample(blurred, samp, in.uv).rgb, t);
    c += textureSample(bloom_tex, samp, in.uv).rgb * p.bloom.z;

    c *= p.grade.x;
    // Warm highlights / cool shadows, like a storybook print.
    let luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
    let warm = vec3(1.0 + 0.08 * p.grade.w, 1.0 + 0.02 * p.grade.w, 1.0 - 0.10 * p.grade.w);
    c *= mix(vec3(1.0), warm, smoothstep(0.05, 0.6, luma));
    c += vec3(0.0, 0.004, 0.012) * p.misc.z * (1.0 - smoothstep(0.0, 0.25, luma));
    // Saturation and contrast (around mid grey).
    let l2 = dot(c, vec3(0.2126, 0.7152, 0.0722));
    c = mix(vec3(l2), c, p.grade.z);
    c = max(vec3(0.0), (c - 0.18) * p.grade.y + 0.18);
    c = shoulder(c);

    // Vignette.
    let v = (in.uv - 0.5) * vec2(p.misc.y, 1.0);
    c *= 1.0 - p.misc.x * smoothstep(0.35, 1.05, length(v));
    return vec4(c, 1.0);
}
