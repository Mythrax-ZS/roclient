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
    // x: focus distance, y: depth blur range, z: fog start, w: fog end
    depth: vec4<f32>,
    // rgb: fog colour, a: fog strength
    fog: vec4<f32>,
    // xy: sun position (uv), z: ray intensity, w: 1 when the sun is in front
    rays: vec4<f32>,
    // x: AO strength, y: sample radius in pixels at the focus distance
    ao: vec4<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> p: Params;
@group(0) @binding(3) var blurred: texture_2d<f32>;
@group(0) @binding(4) var bloom_tex: texture_2d<f32>;
@group(0) @binding(5) var info: texture_2d<f32>;
@group(0) @binding(6) var ao_tex: texture_2d<f32>;
@group(0) @binding(7) var rays_tex: texture_2d<f32>;

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
    let inf = textureSample(info, samp, in.uv);
    let depth = inf.x;
    // Ambient occlusion darkens the world (not sprites, which carry their own shading).
    let ao = textureSample(ao_tex, samp, in.uv).r;
    let sharp = textureSample(src, samp, in.uv).rgb * mix(1.0, ao, 1.0 - inf.y);
    // Depth of field: the tilt-shift band around the player, combined with real distance
    // from the focal plane so tall foreground models and the far map blur too.
    let band = smoothstep(p.dof.z, p.dof.z + p.dof.w, abs(in.uv.y - p.dof.y));
    let away = smoothstep(0.0, p.depth.y, abs(depth - p.depth.x) - p.depth.x * 0.15);
    let t = max(band * 0.7, away) * p.dof.x;
    let blur_ao = mix(1.0, textureSample(ao_tex, samp, in.uv).r, 1.0 - inf.y);
    var c = mix(sharp, textureSample(blurred, samp, in.uv).rgb * blur_ao, t);
    // Distance haze toward the far parts of the view, tinted by the map's ambient light.
    // Nothing drawn there (the void around indoor rooms): leave it dark.
    // (MSAA edges average real depth with the void's 60000, so anything far past the fog
    // range counts as void too.)
    let haze = smoothstep(p.depth.z, p.depth.w, depth) * p.fog.a * select(1.0, 0.0, depth > p.depth.w * 3.0);
    c = mix(c, p.fog.rgb, haze);
    c += textureSample(bloom_tex, samp, in.uv).rgb * p.bloom.z;
    c += textureSample(rays_tex, samp, in.uv).rgb * p.rays.z;

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

// Ambient occlusion from linear depth alone: neighbours clearly nearer the eye than this
// pixel (walls, ledges, props) occlude it. Sprites neither receive nor cast here.
@fragment
fn fs_ao(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(info, samp, in.uv);
    if c.x > 50000.0 || c.y > 0.5 || p.ao.x <= 0.0 {
        return vec4(1.0);
    }
    // A fixed spiral of 12 taps, sized so the radius shrinks with distance.
    let radius = p.ao.y * p.depth.x / max(c.x, 1.0);
    let range = c.x * 0.05;
    var occ = 0.0;
    for (var i = 0; i < 12; i++) {
        let a = f32(i) * 2.39996;
        let r = radius * (0.25 + 0.75 * f32(i) / 11.0);
        let o = vec2(cos(a), sin(a)) * r * p.texel.xy;
        let s = textureSampleLevel(info, samp, in.uv + o, 0.0);
        let dz = c.x - s.x;
        // Closer by a meaningful amount, but not so far it's a separate object.
        occ += select(0.0, smoothstep(range * 0.1, range, dz) * (1.0 - smoothstep(range * 3.0, range * 6.0, dz)), s.y < 0.5);
    }
    return vec4(vec3(1.0 - p.ao.x * occ / 12.0), 1.0);
}

// God rays: march from each pixel toward the sun through the bloom bright pass.
@fragment
fn fs_rays(in: VsOut) -> @location(0) vec4<f32> {
    if p.rays.w < 0.5 || p.rays.z <= 0.0 {
        return vec4(0.0);
    }
    let steps = 24;
    let delta = (p.rays.xy - in.uv) / f32(steps) * 0.85;
    var uv = in.uv;
    var decay = 1.0;
    var sum = vec3(0.0);
    for (var i = 0; i < steps; i++) {
        uv += delta;
        sum += textureSample(src, samp, clamp(uv, vec2(0.0), vec2(1.0))).rgb * decay;
        decay *= 0.93;
    }
    return vec4(sum / f32(steps), 1.0);
}
