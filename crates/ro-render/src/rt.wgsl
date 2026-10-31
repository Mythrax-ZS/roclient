// Ray-traced sun shadows (see rt.rs), joined to scene.wgsl in place of its stub `sun_ray`.
enable wgpu_ray_query;

@group(2) @binding(0) var rt_scene: acceleration_structure;
// Per cut-out triangle: (uv0, uv1), (uv2, mask layer, 0).
@group(2) @binding(1) var<storage, read> rt_cutouts: array<vec4<f32>>;
@group(2) @binding(2) var rt_masks: texture_2d_array<f32>;
@group(2) @binding(3) var rt_samp: sampler;

// 1 when the sun reaches `world` past the map's geometry, 0 when something blocks it. Opaque
// triangles block at once; cut-outs (leaves, fences) only where their texel is solid.
fn sun_ray(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let to_sun = -normalize(g.rt.xyz);
    // Off the surface a little, so it doesn't shadow itself.
    let origin = world + n * 0.3 + to_sun * 0.3;
    var rq: ray_query;
    rayQueryInitialize(&rq, rt_scene, RayDesc(RAY_FLAG_TERMINATE_ON_FIRST_HIT, 0xFFu, 0.01, 5000.0, origin, to_sun));
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
