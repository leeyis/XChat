#[cfg(feature = "desktop")]
fn main() {
    #[cfg(target_os = "windows")]
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());

        let attributes = tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        );
        tauri_build::try_build(attributes).expect("failed to prepare the Tauri build");
    }

    #[cfg(not(target_os = "windows"))]
    tauri_build::build();
}

#[cfg(not(feature = "desktop"))]
fn main() {
    // Web 端不需要 tauri_build
}
