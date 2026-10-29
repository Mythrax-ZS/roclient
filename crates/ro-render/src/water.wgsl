// Water surface. Every feature is a run-time bit in `w.flags.x` (see water.rs `flag`), so
// the quality presets switch without recompiling. Lighting is linear; the HDR target and
// post stack (bloom, fog, grading) take it from there.
//
// Inputs: group 0 the shared globals; group 1 the map's water (animation frame, uniform,
// detail texture, shore/depth map, sky map); group 2 copies of the opaque scene's colour
// and linear depth (refraction, absorption depth, screen-space reflections).

struct Globals {
    view_proj: mat4x4<f32>,
    focus: vec4<f32>,
    eye: vec4<f32>,
    // xyz: direction the sun travels (y-up world)
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    // x: seconds since start
    params: vec4<f32>,
    light_vp: mat4x4<f32>,
    shadow: vec4<f32>,
    // w, h, 1/w, 1/h
    screen: vec4<f32>,
    sprite_light: vec4<f32>,
    lights: array<vec4<f32>, 64>,
    room: vec4<f32>,
};

struct Water {
    // level (y-up), RSW wave height, RSW wave speed (deg/tick), RSW wave pitch (deg/tile)
    level: vec4<f32>,
    // frames per texture step, map width, map height (world), world units per tile
    map: vec4<f32>,
    // feature bits, SSR steps
    flags: vec4<u32>,
    // absorption rgb (per world unit), depth scale
    absorb: vec4<f32>,
    // scatter rgb, classic texture mix
    scatter: vec4<f32>,
    // foam rgb, intensity
    foam_color: vec4<f32>,
    // falloff, speed, scale, crest
    foam: vec4<f32>,
    // refraction strength, reflection strength, SSR distance, shininess
    optics: vec4<f32>,
    // normal strength, tiling, speed
    normal: vec4<f32>,
    // amplitude, wavelength, steepness, speed
    waves: vec4<f32>,
    // mean sky radiance rgb, sky brightness now
    sky: vec4<f32>,
    // sun rgb, sun intensity
    sun: vec4<f32>,
};

const F_WAVES: u32 = 1u;
const F_NORMAL_MAP: u32 = 2u;
const F_REFRACTION: u32 = 4u;
const F_REFLECTION: u32 = 8u;
const F_SSR: u32 = 16u;
const F_ABSORPTION: u32 = 32u;
const F_FOAM: u32 = 64u;
const F_BICUBIC: u32 = 128u;
const F_CLASSIC: u32 = 256u;
const F_SPECULAR: u32 = 512u;
const F_CAUSTICS: u32 = 1024u;

// Keep in sync with water.rs.
const DEPTH_RANGE: f32 = 64.0;
const SHORE_RANGE: f32 = 128.0;
const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var frame_tex: texture_2d<f32>;
@group(1) @binding(1) var wrap: sampler;
@group(1) @binding(2) var<uniform> w: Water;
// RG normal, B height, A foam pattern (tileable).
@group(1) @binding(3) var detail: texture_2d<f32>;
// R depth / DEPTH_RANGE, G distance to shore / SHORE_RANGE, per GND corner.
@group(1) @binding(4) var shore_map: texture_2d<f32>;
// Equirectangular sky, sRGB.
@group(1) @binding(5) var sky_map: texture_2d<f32>;
@group(1) @binding(6) var clamp_samp: sampler;
@group(2) @binding(0) var scene_color: texture_2d<f32>;
// x: linear distance from the eye.
@group(2) @binding(1) var scene_depth: texture_2d<f32>;

fn on(f: u32) -> bool {
    return (w.flags.x & f) != 0u;
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) tile: vec2<f32>,
    // Analytic wave normal and the crest height above the rest level.
    @location(2) normal: vec3<f32>,
    @location(3) crest: f32,
};

fn shore_uv(xz: vec2<f32>) -> vec2<f32> {
    let corners = vec2<f32>(textureDimensions(shore_map));
    return (xz / w.map.w + 0.5) / corners;
}

// One Gerstner wave: adds displacement and the normal's slope terms.
fn gerstner(p: vec2<f32>, dir: vec2<f32>, len: f32, amp: f32, steep: f32, t: f32, disp: ptr<function, vec3<f32>>, nrm: ptr<function, vec3<f32>>) {
    let k = TAU / len;
    let c = sqrt(9.8 * len / TAU) * 0.35 * w.waves.w;
    let f = k * (dot(dir, p) - c * t);
    let q = steep / (k * amp * 3.0 + 1e-4);
    let cf = cos(f);
    let sf = sin(f);
    *disp += vec3(q * amp * dir.x * cf, amp * sf, q * amp * dir.y * cf);
    *nrm += vec3(-dir.x * k * amp * cf, -q * k * amp * sf, -dir.y * k * amp * cf);
}

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) tile: vec2<f32>) -> VsOut {
    let t = g.params.x;
    // RO's own sine swell (RSW wave height / speed / pitch).
    let ticks = t * 60.0;
    let phase = radians(ticks * w.level.z + (tile.x + tile.y) * w.level.w * 0.5);
    var p = vec3(pos.x, w.level.x + sin(phase) * w.level.y, pos.y);
    var n = vec3(0.0, 1.0, 0.0);
    var crest = 0.0;
    if on(F_WAVES) {
        // Swell calms toward the shore so it never climbs the beach.
        let depth = textureSampleLevel(shore_map, clamp_samp, shore_uv(pos), 0.0).r * DEPTH_RANGE;
        let calm = smoothstep(0.5, 10.0, depth);
        let a = w.waves.x * calm;
        let l = w.waves.y;
        let s = w.waves.z;
        var disp = vec3(0.0);
        var slope = vec3(0.0, 1.0, 0.0);
        gerstner(pos, normalize(vec2(1.0, 0.35)), l, a, s, t, &disp, &slope);
        gerstner(pos, normalize(vec2(-0.45, 1.0)), l * 0.61, a * 0.55, s, t, &disp, &slope);
        gerstner(pos, normalize(vec2(0.7, -0.75)), l * 0.37, a * 0.32, s, t, &disp, &slope);
        gerstner(pos, normalize(vec2(-0.9, -0.2)), l * 0.23, a * 0.18, s, t, &disp, &slope);
        p += disp;
        n = normalize(slope);
        // 0 at rest .. 1 at the very top of all four waves stacked.
        crest = disp.y / max(a * 2.05, 1e-3);
    }
    var o: VsOut;
    o.pos = g.view_proj * vec4(p, 1.0);
    o.world = p;
    o.tile = tile;
    o.normal = n;
    o.crest = crest;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

// Cubic B-spline filtering from four bilinear taps (smooth, no grid artefacts when a
// small texture is stretched far). Gradients come from the caller so mips still apply.
fn bicubic(t: texture_2d<f32>, s: sampler, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(t));
    let p = uv * size - 0.5;
    let f = fract(p);
    let i = floor(p);
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - 3.0 * f + 3.0 * f2 - f3) / 6.0;
    let w1 = (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0;
    let w2 = (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (i - 0.5 + w1 / g0) / size;
    let h1 = (i + 1.5 + w3 / g1) / size;
    let a = textureSampleGrad(t, s, vec2(h0.x, h0.y), ddx, ddy);
    let b = textureSampleGrad(t, s, vec2(h1.x, h0.y), ddx, ddy);
    let c = textureSampleGrad(t, s, vec2(h0.x, h1.y), ddx, ddy);
    let d = textureSampleGrad(t, s, vec2(h1.x, h1.y), ddx, ddy);
    return g0.y * (g0.x * a + g1.x * b) + g1.y * (g0.x * c + g1.x * d);
}

fn filtered(t: texture_2d<f32>, s: sampler, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32> {
    if on(F_BICUBIC) {
        return bicubic(t, s, uv, ddx, ddy);
    }
    return textureSampleGrad(t, s, uv, ddx, ddy);
}

fn sky_uv(d: vec3<f32>) -> vec2<f32> {
    let u = atan2(d.z, d.x) / TAU;
    let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    return vec2(fract(u), v);
}

// Sky radiance in a direction, dimmed with the night.
fn sky(d: vec3<f32>) -> vec3<f32> {
    let c = textureSampleLevel(sky_map, wrap, sky_uv(d), 0.0).rgb;
    return srgb_to_linear(c) * w.sky.w;
}

// Linear eye distance of the opaque scene at a screen uv.
fn scene_dist(uv: vec2<f32>) -> f32 {
    let px = vec2<i32>(clamp(uv, vec2(0.0), vec2(0.9999)) * g.screen.xy);
    return textureLoad(scene_depth, px, 0).x;
}

fn to_screen(p: vec3<f32>) -> vec3<f32> {
    let c = g.view_proj * vec4(p, 1.0);
    let n = c.xyz / c.w;
    return vec3(n.x * 0.5 + 0.5, 0.5 - n.y * 0.5, c.w);
}

// Short screen-space reflection: march the reflected ray and take the scene where it
// passes behind visible geometry. Returns colour and confidence (0 = use the sky).
fn ssr(origin: vec3<f32>, dir: vec3<f32>) -> vec4<f32> {
    let steps = max(w.flags.y, 1u);
    let step_len = w.optics.z / f32(steps);
    var t = step_len * 0.5;
    for (var i = 0u; i < steps; i++) {
        let q = origin + dir * t;
        let s = to_screen(q);
        if s.z <= 0.0 || any(s.xy < vec2(0.0)) || any(s.xy > vec2(1.0)) {
            break;
        }
        let behind = distance(g.eye.xyz, q) - scene_dist(s.xy);
        if behind > 0.5 && behind < step_len * 2.0 {
            let edge = min(min(s.x, 1.0 - s.x), min(s.y, 1.0 - s.y));
            let fade = smoothstep(0.0, 0.08, edge) * (1.0 - f32(i) / f32(steps));
            return vec4(textureSampleLevel(scene_color, clamp_samp, s.xy, 0.0).rgb, fade);
        }
        t += step_len;
    }
    return vec4(0.0);
}

// Reserved hook for caustics on the refracted floor (F_CAUSTICS): projected animated
// pattern scaled by depth; returns 0 until implemented.
fn caustics(floor_pos: vec3<f32>, depth: f32) -> f32 {
    return 0.0;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = g.params.x;
    let p = in.world;
    let to_eye = g.eye.xyz - p;
    let eye_dist = length(to_eye);
    let v = to_eye / eye_dist;
    let screen_uv = in.pos.xy * g.screen.zw;

    // Derivatives up front (uniform control flow) for the gradient samples below.
    let tiling = w.normal.y;
    let uv_a = p.xz / tiling + vec2(0.021, 0.013) * t * w.normal.z;
    let uv_b = p.xz / (tiling * 0.47) + vec2(-0.017, 0.024) * t * w.normal.z;
    let da = mat2x2<f32>(dpdx(uv_a), dpdy(uv_a));
    let db = mat2x2<f32>(dpdx(uv_b), dpdy(uv_b));
    let suv = shore_uv(p.xz);
    let ds = mat2x2<f32>(dpdx(suv), dpdy(suv));
    let foam_scale = w.foam.z;
    let uv_f1 = p.xz / foam_scale + vec2(0.03, 0.02) * t * w.foam.y;
    let uv_f2 = p.xz / (foam_scale * 0.71) - vec2(0.025, -0.018) * t * w.foam.y;
    let df1 = mat2x2<f32>(dpdx(uv_f1), dpdy(uv_f1));
    let df2 = mat2x2<f32>(dpdx(uv_f2), dpdy(uv_f2));
    let classic_uv = in.tile / 4.0;
    let dc = mat2x2<f32>(dpdx(classic_uv), dpdy(classic_uv));

    // Surface normal: swell + two scrolling ripple layers, flattened with distance.
    var n = normalize(in.normal);
    if on(F_NORMAL_MAP) {
        let a = filtered(detail, wrap, uv_a, da[0], da[1]).rg * 2.0 - 1.0;
        let b = filtered(detail, wrap, uv_b, db[0], db[1]).rg * 2.0 - 1.0;
        let fade = 1.0 / (1.0 + eye_dist / 900.0);
        let ripple = (a + b * 0.6) * w.normal.x * fade;
        n = normalize(vec3(n.x + ripple.x, n.y, n.z + ripple.y));
    }

    // Depth: the baked vertical depth everywhere, and the view-ray thickness from the
    // opaque scene where the screen copy is available.
    let shore = filtered(shore_map, clamp_samp, suv, ds[0], ds[1]);
    let vertical = shore.r * DEPTH_RANGE;
    let screen_ok = on(F_REFRACTION) || on(F_SSR);
    var thickness = vertical / max(v.y, 0.15);
    var refr_uv = screen_uv;
    if on(F_REFRACTION) {
        thickness = max(scene_dist(screen_uv) - eye_dist, 0.0);
        // Bend the view by the normal tilt, more where the water is deeper; reject
        // samples that land on things in front of the water.
        let bend = n.xz * w.optics.x * clamp(thickness / 12.0, 0.0, 1.0);
        let cand = clamp(screen_uv + bend, vec2(0.001), vec2(0.999));
        let cand_thick = scene_dist(cand) - eye_dist;
        if cand_thick > 0.0 {
            refr_uv = cand;
            thickness = cand_thick;
        }
    }
    let depth = thickness * w.absorb.w;

    // Lighting terms.
    let sun_dir = -normalize(g.light_dir.xyz);
    let sun = w.sun.rgb;
    let mean_sky = w.sky.rgb * w.sky.w;
    let ambient = g.ambient.rgb + mean_sky * 0.5;

    // In-scattered colour of the water body (lit by sky + sun), optionally tinted by RO's
    // own animated texture.
    var body = w.scatter.rgb * (ambient + sun * max(sun_dir.y, 0.0) * 0.6);
    if on(F_CLASSIC) {
        let bent = classic_uv + n.xz * 0.02;
        let classic = srgb_to_linear(filtered(frame_tex, wrap, bent, dc[0], dc[1]).rgb) * (g.ambient.rgb + g.diffuse.rgb * 0.6);
        body = mix(body, classic, w.scatter.w);
    }

    // Beer-Lambert: what survives the trip through `depth` world units of water.
    var transmit = vec3(1.0);
    if on(F_ABSORPTION) {
        transmit = exp(-w.absorb.rgb * depth);
    } else {
        transmit = vec3(exp(-depth * 0.02));
    }

    var color: vec3<f32>;
    var alpha = 1.0;
    if on(F_REFRACTION) {
        let below = textureSampleLevel(scene_color, clamp_samp, refr_uv, 0.0).rgb;
        var floor_light = below;
        if on(F_CAUSTICS) {
            floor_light += below * caustics(p - v * thickness, depth);
        }
        color = floor_light * transmit + body * (1.0 - transmit);
    } else {
        // No scene copy: blend over the ground, more opaque where deeper.
        color = body;
        alpha = clamp(1.0 - dot(transmit, vec3(0.333)), 0.35, 0.95);
        if !on(F_ABSORPTION) {
            alpha = 0.56;
        }
    }

    // Reflection: Schlick Fresnel over the sky (or the scene, via SSR).
    let ndv = clamp(dot(n, v), 0.0, 1.0);
    let fresnel = (0.02 + 0.98 * pow(1.0 - ndv, 5.0)) * w.optics.y;
    if on(F_REFLECTION) {
        let r = reflect(-v, n);
        var refl = sky(vec3(r.x, max(r.y, 0.02), r.z));
        if on(F_SSR) && screen_ok {
            let hit = ssr(p + n * 0.5, r);
            refl = mix(refl, hit.rgb, hit.a);
        }
        color = mix(color, refl, fresnel);
        alpha = max(alpha, fresnel);
    } else {
        // Mean sky radiance stands in for the sky map.
        color = mix(color, mean_sky, fresnel);
    }

    // Sun glitter.
    if on(F_SPECULAR) {
        let h = normalize(sun_dir + v);
        let spec = pow(max(dot(n, h), 0.0), w.optics.w) * w.sun.w * smoothstep(0.0, 0.1, sun_dir.y);
        color += sun * spec;
        alpha = max(alpha, clamp(spec, 0.0, 1.0));
    }

    // Shore foam: where the water thins out (baked depth, or the true view thickness),
    // broken up by two scrolling foam patterns; a little on wave crests too.
    if on(F_FOAM) {
        let f1 = filtered(detail, wrap, uv_f1, df1[0], df1[1]).a;
        let f2 = filtered(detail, wrap, uv_f2, df2[0], df2[1]).a;
        let pattern = clamp(f1 * 0.65 + f2 * 0.55, 0.0, 1.0);
        // RO seabeds sit only a little under the surface almost everywhere, so foam keys off
        // the distance to the shoreline (baked) rather than depth, plus a thin contact line
        // where the water meets piers and walls (from the scene depth).
        var near_shore = 1.0 - smoothstep(0.0, w.foam.x, shore.g * SHORE_RANGE);
        if on(F_REFRACTION) {
            near_shore = max(near_shore, 1.0 - smoothstep(0.0, 1.6, thickness * max(v.y, 0.15)));
        }
        let lapping = 0.5 + 0.5 * sin(t * 1.7 * w.foam.y + shore.g * SHORE_RANGE * 0.35);
        var foam = smoothstep(1.0 - near_shore, 1.15 - near_shore * 0.5, pattern * (0.75 + 0.25 * lapping));
        foam = max(foam, smoothstep(0.75, 1.0, near_shore) * 0.85);
        // Crests: only the tallest peaks, as thin streaks (the pattern's brightest lines),
        // thinned further by a slow large-scale mask so open water stays mostly clear.
        let mask = filtered(detail, wrap, p.xz / (foam_scale * 9.0) + vec2(0.004, 0.003) * t, df1[0] / 9.0, df1[1] / 9.0).b;
        let crest = smoothstep(0.6, 0.9, in.crest) * smoothstep(0.6, 0.85, mask);
        foam = max(foam, crest * w.foam.w * smoothstep(0.6, 0.95, pattern));
        foam *= w.foam_color.a;
        let lit = w.foam_color.rgb * (g.ambient.rgb + g.diffuse.rgb * max(sun_dir.y, 0.2));
        color = mix(color, lit, foam);
        alpha = max(alpha, foam);
    }
    return vec4(color, alpha);
}
