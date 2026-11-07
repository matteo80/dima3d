//! Incorpora nell'exe l'icona e il manifest di Windows (DPI per monitor, controlli moderni).

fn main() {
    println!("cargo:rerun-if-changed=assets/icona.ico");
    println!("cargo:rerun-if-changed=assets/gestore-wrl.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icona.ico")
        .set_manifest_file("assets/gestore-wrl.manifest")
        .set("FileDescription", "Gestore WRL")
        .set("ProductName", "Gestore WRL");
    // Senza il compilatore di risorse l'exe funziona lo stesso, solo senza icona e manifest
    if let Err(e) = res.compile() {
        println!("cargo:warning=Icona e manifest non incorporati: {e}");
    }
}
