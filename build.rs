//! Incorpora nell'exe l'icona e il manifest di Windows (DPI per monitor, controlli moderni).

fn main() {
    println!("cargo:rerun-if-changed=assets/icona.ico");
    println!("cargo:rerun-if-changed=assets/dima3d.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icona.ico")
        .set_manifest_file("assets/dima3d.manifest")
        .set("FileDescription", "Dima 3D — visualizzatore e posizionatore di matematiche 3D")
        .set("ProductName", "Dima 3D")
        .set("LegalCopyright", "Licenza MIT — codice scritto interamente con AI (Claude, Anthropic)");
    // Senza il compilatore di risorse l'exe funziona lo stesso, solo senza icona e manifest
    if let Err(e) = res.compile() {
        println!("cargo:warning=Icona e manifest non incorporati: {e}");
    }
}
