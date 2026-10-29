// A painted sky panorama for RO's tilted diorama camera. The real camera looks down,
// so a physical horizon would be above the screen: we deliberately show a broader
// band of sky behind the map instead. Rotation changes the vista; walking and zooming
// do not. All colour inputs and outputs are linear.

struct Sky {
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    cloud: vec4<f32>,
    // Cloud cover, scale, wind speed, haze.
    clouds: vec4<f32>,
    // Seconds, night blend, star strength, map seed.
    time: vec4<f32>,
    // Camera yaw, pitch, aspect, vertical field of view.
    camera: vec4<f32>,
    sun: vec4<f32>,
};

@group(0) @binding(0) var<uniform> s: Sky;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VsOut;
    out.pos = vec4(xy * 2.0 - 1.0, 1.0, 1.0);
    out.uv = vec2(xy.x, 1.0 - xy.y);
    return out;
}

// Arithmetic hashes avoid the precision-dependent streaks of sin-based noise.
fn hash(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.yzx + vec3(33.33));
    return fract((q.x + q.y) * q.z);
}

fn noise(p: vec3<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let t = f * f * (3.0 - 2.0 * f);
    let z0 = mix(
        mix(hash(cell), hash(cell + vec3(1.0, 0.0, 0.0)), t.x),
        mix(hash(cell + vec3(0.0, 1.0, 0.0)), hash(cell + vec3(1.0, 1.0, 0.0)), t.x), t.y);
    let z1 = mix(
        mix(hash(cell + vec3(0.0, 0.0, 1.0)), hash(cell + vec3(1.0, 0.0, 1.0)), t.x),
        mix(hash(cell + vec3(0.0, 1.0, 1.0)), hash(cell + vec3(1.0, 1.0, 1.0)), t.x), t.y);
    return mix(z0, z1, t.z);
}

fn cloud_noise(p: vec3<f32>) -> f32 {
    // Broad, soft masses with just enough small variation for a painted edge.
    return noise(p) * 0.56
        + noise(p * 2.03 + vec3(11.4, 7.1, 3.8)) * 0.28
        + noise(p * 4.11 + vec3(4.2, 13.5, 9.2)) * 0.12
        + noise(p * 8.23 + vec3(19.6, 5.2, 15.7)) * 0.04;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) info: vec2<f32>,
};

@fragment
fn fs_main(input: VsOut) -> FsOut {
    let uv = input.uv;
    let night = s.time.y;
    let seed = s.time.w;
    let haze = s.clouds.w;
    // A wider lens only for the backdrop prevents cloud shapes stretching at ultrawide
    // aspect ratios. Yaw wraps seamlessly because noise is sampled on a cylinder.
    let lens = max(tan(s.camera.w * 0.5), 0.62);
    let bearing = -s.camera.x + atan((uv.x - 0.5) * 2.0 * lens * s.camera.z);
    let elevation = 0.08 + (1.0 - uv.y) * 0.82 + (0.872665 - s.camera.y) * 0.3;
    let direction = vec3(sin(bearing) * cos(elevation), sin(elevation), cos(bearing) * cos(elevation));

    let height = clamp((elevation - 0.08) / 0.82, 0.0, 1.0);
    let gradient = pow(height, 0.7 + haze * 0.4);
    let day_color = mix(s.horizon.rgb, s.zenith.rgb, gradient);
    // Keep each map's hue in moonlight rather than replacing every sky with the same blue.
    let night_top = s.zenith.rgb * vec3(0.06, 0.08, 0.17) + vec3(0.004, 0.007, 0.018);
    let night_bottom = s.horizon.rgb * vec3(0.08, 0.10, 0.18) + vec3(0.008, 0.012, 0.027);
    var color = mix(day_color, mix(night_bottom, night_top, gradient), night);

    let sun_alignment = max(dot(direction, s.sun.xyz), 0.0);
    let sun_glow = pow(sun_alignment, 28.0) * (1.0 - haze * 0.65);
    color += s.cloud.rgb * sun_glow * 0.13 * (1.0 - night);

    let drift = s.time.x * s.clouds.z;
    // Compress the cloud shapes vertically into broad banks instead of tall pillars.
    let cloud_pos = vec3(cos(bearing + drift * 0.025) * 2.6, sin(bearing + drift * 0.025) * 2.6, elevation * 5.6)
        * s.clouds.y + vec3(seed * 47.3, seed * 19.7, seed * 83.1 + drift * 0.075);
    let mass = cloud_noise(cloud_pos);
    let threshold = 0.72 - s.clouds.x * 0.42;
    let density = smoothstep(threshold, threshold + 0.19, mass) * smoothstep(0.0, 0.12, s.clouds.x);
    let edge_light = smoothstep(threshold + 0.04, threshold + 0.34, mass);
    let shadow = mix(s.horizon.rgb, s.cloud.rgb, 0.55);
    let day_cloud = mix(shadow, s.cloud.rgb, edge_light) + s.cloud.rgb * sun_glow * 0.12;
    let night_cloud = mix(night_bottom, s.cloud.rgb * vec3(0.08, 0.10, 0.17), edge_light);
    // Clouds settle into the horizon haze and become gentler at night.
    let opacity = density * mix(0.70, 0.96, smoothstep(0.0, 0.32, height)) * (1.0 - night * 0.15);
    color = mix(color, mix(day_cloud, night_cloud, night), opacity);

    // A sparse, antialiased star field, tied to map bearing rather than screen pixels.
    // Wrapping the longitude cell keeps the same stars when rotating across north.
    let star_pos = vec2(bearing / 6.2831853 * 384.0, elevation * 140.0);
    let star_cell = floor(star_pos);
    let wrapped_x = star_cell.x - floor(star_cell.x / 384.0) * 384.0;
    let star_id = vec3(wrapped_x, star_cell.y, floor(seed * 997.0));
    let star_hash = hash(star_id);
    let star_offset = vec2(hash(star_id + vec3(17.0)), hash(star_id + vec3(39.0))) * 0.6 + 0.2;
    let radius = 0.045 + hash(star_id + vec3(71.0)) * 0.055;
    let aa = max(length(fwidth(star_pos)) * 0.5, 0.005);
    let dot_shape = 1.0 - smoothstep(radius, radius + aa, length(fract(star_pos) - star_offset));
    let twinkle = 0.86 + 0.14 * sin(s.time.x * 0.7 + star_hash * 125.0);
    let stars = step(0.991, star_hash) * dot_shape * twinkle * s.time.z * night
        * (1.0 - opacity) * smoothstep(0.04, 0.35, height) * (1.0 - haze * 0.6);
    color += vec3(0.62, 0.73, 0.95) * stars;

    var out: FsOut;
    out.color = vec4(max(color, vec3(0.0)), 1.0);
    // Same void distance as the renderer clear: AO and geometry fog ignore the sky.
    // Depth writes are disabled, so geometry, sprites and transparent effects cover it.
    out.info = vec2(60000.0, 0.0);
    return out;
}
