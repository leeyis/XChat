#[cfg(feature = "desktop")]
fn main() {
    // 构建脚本里的 #[cfg(target_os = ...)] 判断的是*宿主机*，不是编译目标。
    // 在 Windows 上交叉编译 Android 时会误把 MSVC 专属的 /MANIFESTINPUT 链接参数
    // 传给 clang，因此这里改读 CARGO_CFG_TARGET_OS。
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "windows" {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());

        let attributes = tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        );
        tauri_build::try_build(attributes).expect("failed to prepare the Tauri build");
    } else {
        tauri_build::build();
    }
}

#[cfg(not(feature = "desktop"))]
fn main() {
    // Web 端不需要 tauri_build
}
