// Ray-traced sun shadows (see rt.rs), joined to the world and sprite shaders in place of their
// stub `sun_ray`.
enable wgpu_ray_query;

@group(2) @binding(0) var rt_scene: acceleration_structure;
// Per cut-out triangle: (uv0, uv1), (uv2, mask layer, 0).
@group(2) @binding(1) var<storage, read> rt_cutouts: array<vec4<f32>>;
@group(2) @binding(2) var rt_masks: texture_2d_array<f32>;
@group(2) @binding(3) var rt_samp: sampler;

/// The sun's radius as a slope off its centre (~1.7°): wider than the real sun, for the soft,
/// painterly edges of the sprite shadows.
const SUN_RADIUS: f32 = 0.03;

// How much of the sun reaches `world` past the map's geometry (1 all .. 0 none), from `rays`
// rays spread over its disc; 1 where ray-traced shadows are off.
fn sun_ray(world: vec3<f32>, n: vec3<f32>, rays: u32) -> f32 {
    if g.rt.w < 0.5 {
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
        let c = rayQueryGetCandidateIntersection(&rq);
        let a = rt_cutouts[c.primitive_index * 2u];
        let b = rt_cutouts[c.primitive_index * 2u + 1u];
        let w = c.barycentrics;
        let uv = a.xy * (1.0 - w.x - w.y) + a.zw * w.x + b.xy * w.y;
        if textureSampleLevel(rt_masks, rt_samp, uv, i32(b.z), 0.0).r >= 0.5 {
            rayQueryConfirmIntersection(&rq);
        }
    }
    return select(1.0, 0.0, rayQueryGetCommittedIntersection(&rq).kind != RAY_QUERY_INTERSECTION_NONE);
}
