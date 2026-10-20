fn main() {
    // Tauri links its Windows manifest (Common Controls v6) into the app binary
    // only; test binaries without it do not start (0xc0000139). So the manifest
    // goes into every binary of this crate instead.
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    if let Err(e) = tauri_build::try_build(attributes) {
        panic!("tauri-build: {e:#}");
    }
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if msvc {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        let manifest = std::path::Path::new(&dir).join("windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
