//! Converts RO textures to PNG beside them: cargo run -p ro-formats --example topng -- file...
fn main() -> anyhow::Result<()> {
    for p in std::env::args().skip(1) {
        let img = ro_formats::image::decode(&std::fs::read(&p)?, &p)?;
        image::save_buffer(format!("{p}.png"), &img.pixels, img.width, img.height, image::ExtendedColorType::Rgba8)?;
    }
    Ok(())
}
