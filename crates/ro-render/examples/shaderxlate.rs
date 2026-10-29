// Translates a WGSL shader to SPIR-V, GLSL and HLSL with naga (the compiler wgpu uses),
// so the WGSL stays the single source: shaderxlate <file.wgsl> <out dir>
//
//   cargo run -p ro-render --example shaderxlate -- crates/ro-render/src/water.wgsl target/shaders
//
// Writes <name>.spv, <name>.<entry>.vert/.frag (GLSL 4.50 core) and <name>.hlsl (SM 5.1).
use naga::back::{glsl, hlsl, spv};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (src_path, out) = (&args[1], std::path::PathBuf::from(&args[2]));
    let src = std::fs::read_to_string(src_path).unwrap();
    let name = std::path::Path::new(src_path).file_stem().unwrap().to_string_lossy().into_owned();
    let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).expect("valid WGSL");
    std::fs::create_dir_all(&out).unwrap();

    let words = spv::write_vec(&module, &info, &spv::Options::default(), None).expect("SPIR-V");
    std::fs::write(out.join(format!("{name}.spv")), bytemuck_words(&words)).unwrap();

    for ep in &module.entry_points {
        let (stage, ext) = match ep.stage {
            naga::ShaderStage::Vertex => (naga::ShaderStage::Vertex, "vert"),
            naga::ShaderStage::Fragment => (naga::ShaderStage::Fragment, "frag"),
            _ => continue,
        };
        let options = glsl::Options { version: glsl::Version::Desktop(450), ..Default::default() };
        let pipeline = glsl::PipelineOptions { shader_stage: stage, entry_point: ep.name.clone(), multiview: None };
        let mut text = String::new();
        let mut w = glsl::Writer::new(&mut text, &module, &info, &options, &pipeline, naga::proc::BoundsCheckPolicies::default()).expect("GLSL writer");
        w.write().expect("GLSL");
        std::fs::write(out.join(format!("{name}.{}.{ext}", ep.name)), text).unwrap();
    }

    let mut text = String::new();
    let options = hlsl::Options { shader_model: hlsl::ShaderModel::V5_1, ..Default::default() };
    let pipeline = hlsl::PipelineOptions::default();
    let mut w = hlsl::Writer::new(&mut text, &options, &pipeline);
    w.write(&module, &info, None).expect("HLSL");
    std::fs::write(out.join(format!("{name}.hlsl")), text).unwrap();
    println!("wrote {name}.spv, GLSL per entry point and {name}.hlsl to {}", out.display());
}

fn bytemuck_words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}
