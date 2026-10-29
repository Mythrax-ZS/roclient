// Counts the light sources and prints the sun of each RSW given: rswlights <file.rsw>...
fn main() {
    for p in std::env::args().skip(1) {
        let d = std::fs::read(&p).unwrap();
        match ro_formats::rsw::Rsw::parse(&d) {
            Ok(r) => println!("{p}: lights={} sun lon={} lat={}", r.lights.len(), r.light.longitude, r.light.latitude),
            Err(e) => println!("{p}: {e:#}"),
        }
    }
}
