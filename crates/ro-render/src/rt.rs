//! Ray-traced sun shadows: the map's static geometry in an acceleration structure that the
//! world shader casts a ray through toward the sun (wgpu ray queries; GPUs with ray tracing).
//! Cut-out texels (leaves, fences, railings) let the light through.

use ro_formats::image::Rgba;
use wgpu::util::DeviceExt;

use crate::scene::Vertex;

/// Edge of a cut-out mask layer: textures are sampled down to this for the alpha test.
const MASK: u32 = 256;

/// The device features and limits ray tracing needs, if the adapter has them.
pub fn device_needs(adapter: &wgpu::Adapter) -> Option<(wgpu::Features, wgpu::Limits)> {
    // Phones: not worth the battery, and the drivers are the least proven.
    let rq = wgpu::Features::EXPERIMENTAL_RAY_QUERY;
    (cfg!(not(target_os = "android")) && adapter.features().contains(rq)).then(|| (rq, wgpu::Limits::default().using_acceleration_structure_values(adapter.limits())))
}

pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ray tracing"),
        entries: &[
            entry(0, wgpu::BindingType::AccelerationStructure { vertex_return: false }),
            entry(1, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
            entry(2, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false }),
            entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
        ],
    })
}

/// The scene's acceleration structure and cut-out data, bound as group 2 of the world shader.
/// `textures[i]` is the image used by `ranges[i]` of `vertices`.
pub fn build(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, vertices: &[Vertex], ranges: &[std::ops::Range<u32>], textures: &[Option<&Rgba>]) -> wgpu::BindGroup {
    let (opaque, opaque_n, cut_info, masks) = split(vertices, ranges, textures, device.limits().max_texture_array_layers as usize);
    upload(device, queue, layout, opaque, opaque_n, cut_info, masks)
}

/// Opaque triangles first, then cut-out ones (each with its UVs and mask layer), so each is
/// one BLAS geometry and only the cut-outs run the alpha test. Returns the positions, how
/// many are opaque, the cut-out data and the masks (MASK² bytes per layer); none is empty.
fn split(vertices: &[Vertex], ranges: &[std::ops::Range<u32>], textures: &[Option<&Rgba>], max_layers: usize) -> (Vec<[f32; 3]>, u32, Vec<[f32; 4]>, Vec<u8>) {
    let (mut opaque, mut cut, mut cut_info, mut masks) = (vec![], vec![], vec![], vec![]);
    for (range, img) in ranges.iter().zip(textures) {
        let tris = &vertices[range.start as usize..range.end as usize];
        // shortcut: past the GPU's layer limit (256+) textures count as opaque; fine for RO maps.
        let keyed = img.filter(|i| i.pixels.chunks_exact(4).any(|p| p[3] < 128) && (masks.len() / (MASK * MASK) as usize) < max_layers);
        let Some(img) = keyed else {
            opaque.extend(tris.iter().map(|v| v.pos));
            continue;
        };
        let layer = (masks.len() / (MASK * MASK) as usize) as f32;
        masks.extend((0..MASK * MASK).map(|i| {
            let (x, y) = ((i % MASK) * img.width / MASK, (i / MASK) * img.height / MASK);
            img.pixels[((y * img.width + x) * 4 + 3) as usize]
        }));
        for t in tris.chunks_exact(3) {
            cut.extend(t.iter().map(|v| v.pos));
            cut_info.extend([[t[0].uv[0], t[0].uv[1], t[1].uv[0], t[1].uv[1]], [t[2].uv[0], t[2].uv[1], layer, 0.0]]);
        }
    }
    // Something to trace even on an empty map: a triangle far below it.
    if opaque.is_empty() && cut.is_empty() {
        opaque = vec![[0.0, -1e6, 0.0], [1.0, -1e6, 0.0], [0.0, -1e6, 1.0]];
    }
    if cut_info.is_empty() {
        cut_info.push([0.0; 4]);
    }
    if masks.is_empty() {
        masks.resize((MASK * MASK) as usize, 255);
    }
    let opaque_n = opaque.len() as u32;
    opaque.extend(cut);
    (opaque, opaque_n, cut_info, masks)
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, opaque: Vec<[f32; 3]>, opaque_n: u32, cut_info: Vec<[f32; 4]>, masks: Vec<u8>) -> wgpu::BindGroup {
    let positions = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rt positions"),
        contents: bytemuck::cast_slice(&opaque),
        usage: wgpu::BufferUsages::BLAS_INPUT,
    });
    let sizes: Vec<_> = [(opaque_n, wgpu::AccelerationStructureGeometryFlags::OPAQUE), (opaque.len() as u32 - opaque_n, wgpu::AccelerationStructureGeometryFlags::empty())]
        .into_iter()
        .scan(0, |first, (count, flags)| {
            let at = *first;
            *first += count;
            Some((at, wgpu::BlasTriangleGeometrySizeDescriptor { vertex_format: wgpu::VertexFormat::Float32x3, vertex_count: count, index_format: None, index_count: None, flags }))
        })
        .filter(|(_, s)| s.vertex_count > 0)
        .collect();
    let blas = device.create_blas(
        &wgpu::CreateBlasDescriptor { label: Some("map"), flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE, update_mode: wgpu::AccelerationStructureUpdateMode::Build },
        wgpu::BlasGeometrySizeDescriptors::Triangles { descriptors: sizes.iter().map(|(_, s)| s.clone()).collect() },
    );
    let mut tlas = device.create_tlas(&wgpu::CreateTlasDescriptor {
        label: Some("map"),
        max_instances: 1,
        flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
        update_mode: wgpu::AccelerationStructureUpdateMode::Build,
    });
    tlas[0] = Some(wgpu::TlasInstance::new(&blas, [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0], 0, 0xFF));
    let geometry = sizes
        .iter()
        .map(|(first, size)| wgpu::BlasTriangleGeometry {
            size,
            vertex_buffer: &positions,
            first_vertex: *first,
            vertex_stride: 12,
            index_buffer: None,
            first_index: None,
            transform_buffer: None,
            transform_buffer_offset: None,
        })
        .collect();
    let mut enc = device.create_command_encoder(&Default::default());
    enc.build_acceleration_structures([&wgpu::BlasBuildEntry { blas: &blas, geometry: wgpu::BlasGeometries::TriangleGeometries(geometry) }], [&tlas]);
    queue.submit([enc.finish()]);

    let info = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("rt cut-outs"), contents: bytemuck::cast_slice(&cut_info), usage: wgpu::BufferUsages::STORAGE });
    let layers = (masks.len() / (MASK * MASK) as usize) as u32;
    let masks = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("rt masks"),
            size: wgpu::Extent3d { width: MASK, height: MASK, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &masks,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ray tracing"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::AccelerationStructure(&tlas) },
            wgpu::BindGroupEntry { binding: 1, resource: info.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&masks.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() })) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(z: f32) -> [Vertex; 3] {
        [0.0, 1.0, 2.0].map(|u| Vertex { pos: [u, 0.0, z], normal: [0.0; 3], uv: [u, z], lightmap_uv: [0.0; 2], color: [1.0; 3] })
    }

    #[test]
    fn cut_outs_go_last_with_their_uvs_and_mask() {
        let solid = Rgba { width: 2, height: 2, pixels: vec![255; 16] };
        // Left column see-through.
        let keyed = Rgba { width: 2, height: 2, pixels: (0..4).flat_map(|i| [9, 9, 9, if i % 2 == 0 { 0 } else { 255 }]).collect() };
        let vertices: Vec<Vertex> = [tri(1.0), tri(2.0), tri(3.0)].concat();
        let (pos, opaque, info, masks) = split(&vertices, &[0..3, 3..6, 6..9], &[Some(&keyed), Some(&solid), None], 8);
        assert_eq!(opaque, 6);
        assert_eq!(pos.len(), 9);
        assert_eq!(pos[6][2], 1.0, "the keyed triangle comes after the opaque ones");
        assert_eq!(info, vec![[0.0, 1.0, 1.0, 1.0], [2.0, 1.0, 0.0, 0.0]]);
        assert_eq!(masks.len(), (MASK * MASK) as usize);
        assert_eq!((masks[0], masks[(MASK - 1) as usize]), (0, 255));
        // Past the layer limit a keyed texture counts as opaque.
        assert_eq!(split(&vertices, &[0..3, 3..6, 6..9], &[Some(&keyed), Some(&solid), None], 0).1, 9);
    }
}
