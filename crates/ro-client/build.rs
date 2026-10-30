//! Gives the Windows exe its icon (assets/tenseiga.ico, drawn by packaging/logo.py).

fn main() {
    println!("cargo:rerun-if-changed=assets/tenseiga.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/tenseiga.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no exe icon: {e}");
        }
    }
}
