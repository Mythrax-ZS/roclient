// Ray-traced sun shadows and reflections (see rt.rs), joined to the world, sprite and water
// shaders in place of their stubs (`sun_ray`, `rt_reflect`).
enable wgpu_ray_query;

@group(2) @binding(0) var rt_scene: acceleration_structure;
// Entry 0: x the number of opaque triangles (they come first). Then per triangle (rt.rs
// `split`): (uv0, uv1), (uv2, texture layer or -1, 0), (colour, 0), (normal, 0).
@group(2) @binding(1) var<storage, read> rt_info: array<vec4<f32>>;
// The map's textures, small (alpha for the cut-out test, colour for reflections).
@group(2) @binding(2) var rt_textures: texture_2d_array<f32>;
@group(2) @binding(3) var rt_samp: sampler;

/// The sun's radius as a slope off its centre (~1.7°): wider than the real sun, for the soft,
/// painterly edges of the sprite shadows.
const SUN_RADIUS: f32 = 0.03;

// How much of the sun reaches `world` past the map's geometry (1 all .. 0 none), from `rays`
// rays spread over its disc; 1 where ray-traced shadows are off.
fn sun_ray(world: vec3<f32>, n: vec3<f32>, rays: u32) -> f32 {
    if (u32(g.rt.w) & 1u) == 0u {
        return 1.0;
    }
    let to_sun = -normalize(g.rt.xyz);
    // Off the surface a little, so it doesn't shadow itself.
    let origin = world + n * 0.3 + to_sun * 0.3;
    let side = normalize(cross(to_sun, select(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), abs(to_sun.y) > 0.9)));
    let up = cross(to_sun, side);
    // The pattern turns from spot to spot (fixed in the world, so it doesn't crawl as the
    // camera moves): a few rays then read as a smooth penumbra.
    let c = vec3<u32>(vec3<i32>(floor(world * 4.0)));
    var h = (c.x * 73856093u) ^ (c.y * 19349663u) ^ (c.z * 83492791u);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    let spin = f32(h ^ (h >> 16u)) / 4294967296.0 * 6.2831853;
    var lit = 0.0;
    for (var i = 0u; i < rays; i++) {
        let a = spin + f32(i) * 6.2831853 / f32(rays);
        let r = select(SUN_RADIUS * sqrt((f32(i) + 0.5) / f32(rays)), 0.0, rays == 1u);
        lit += clear_to(origin, normalize(to_sun + (side * cos(a) + up * sin(a)) * r));
    }
    return lit / f32(rays);
}

// 1 when nothing blocks the ray. Opaque triangles block at once; cut-outs (leaves, fences)
// only where their texel is solid.
fn clear_to(origin: vec3<f32>, dir: vec3<f32>) -> f32 {
    var rq: ray_query;
    rayQueryInitialize(&rq, rt_scene, RayDesc(RAY_FLAG_TERMINATE_ON_FIRST_HIT, 0xFFu, 0.01, 5000.0, origin, dir));
    while rayQueryProceed(&rq) {
        if solid(rayQueryGetCandidateIntersection(&rq)) {
            rayQueryConfirmIntersection(&rq);
        }
    }
    return select(1.0, 0.0, rayQueryGetCommittedIntersection(&rq).kind != RAY_QUERY_INTERSECTION_NONE);
}

// Whether a cut-out candidate (the only kind that comes up) is solid where the ray crosses it.
// (The loop around it is written out in each caller: naga can't pass a ray query along.)
fn solid(c: RayIntersection) -> bool {
    return surface(u32(rt_info[0].x) + c.primitive_index, c.barycentrics).a >= 0.5;
}

// A triangle's texture (small copy) at barycentrics `w`; grey where it has none.
fn surface(tri: u32, w: vec2<f32>) -> vec4<f32> {
    let a = rt_info[1u + tri * 4u];
    let b = rt_info[2u + tri * 4u];
    let uv = a.xy * (1.0 - w.x - w.y) + a.zw * w.x + b.xy * w.y;
    if b.z < 0.0 {
        return vec4(0.5, 0.5, 0.5, 1.0);
    }
    return textureSampleLevel(rt_textures, rt_samp, uv, i32(b.z), 0.0);
}

// What a reflected ray sees of the map (linear rgb, and 1 where it hit something; 0 for the
// sky): the surface lit by the sun the way the world shader lights it, without lightmaps.
fn rt_reflect(origin: vec3<f32>, dir: vec3<f32>) -> vec4<f32> {
    if (u32(g.rt.w) & 2u) == 0u {
        return vec4(0.0);
    }
    var rq: ray_query;
    rayQueryInitialize(&rq, rt_scene, RayDesc(RAY_FLAG_NONE, 0xFFu, 0.01, 5000.0, origin, dir));
    while rayQueryProceed(&rq) {
        if solid(rayQueryGetCandidateIntersection(&rq)) {
            rayQueryConfirmIntersection(&rq);
        }
    }
    let hit = rayQueryGetCommittedIntersection(&rq);
    if hit.kind == RAY_QUERY_INTERSECTION_NONE {
        return vec4(0.0);
    }
    let tri = hit.primitive_index + hit.geometry_index * u32(rt_info[0].x);
    var n = normalize(rt_info[4u + tri * 4u].xyz + vec3(0.0, 1e-5, 0.0));
    n = select(n, -n, dot(n, dir) > 0.0);
    let p = origin + dir * hit.t;
    let light = clamp(g.ambient.rgb + g.diffuse.rgb * max(dot(n, -g.light_dir.xyz), 0.0), vec3(0.0), vec3(1.0));
    let shade = 1.0 - g.shadow.x * (1.0 - sun_ray(p, n, 1u));
    let rgb = surface(tri, hit.barycentrics).rgb * rt_info[3u + tri * 4u].rgb * light;
    return vec4(srgb_to_linear(clamp(rgb, vec3(0.0), vec3(1.0))) * shade, 1.0);
}
