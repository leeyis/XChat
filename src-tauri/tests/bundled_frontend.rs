#![cfg(all(feature = "desktop", feature = "custom-protocol"))]

use tauri::utils::config::FrontendDist;

#[test]
fn packaged_frontend_contains_its_entrypoint_and_referenced_assets() {
    let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    assert!(
        matches!(
            context.config().build.frontend_dist,
            Some(FrontendDist::Directory(_))
        ),
        "packaged Xchat must embed a frontend directory, not navigate to a build-machine URL"
    );

    let index = context
        .assets()
        .get(&"index.html".into())
        .expect("packaged frontend is missing index.html");
    let html = std::str::from_utf8(&index).expect("index.html must be UTF-8");
    assert!(html.contains("id=\"root\""));

    let mut has_script = false;
    let mut has_stylesheet = false;
    for attribute in ["src=\"", "href=\""] {
        for value in html.split(attribute).skip(1) {
            let path = value.split('"').next().unwrap().trim_start_matches('/');
            if path.is_empty() || path.contains(':') || path.starts_with('#') {
                continue;
            }
            let asset = context
                .assets()
                .get(&path.into())
                .unwrap_or_else(|| panic!("index.html references missing bundled asset: {path}"));
            assert!(!asset.is_empty(), "bundled asset is empty: {path}");
            has_script |= path.starts_with("assets/") && path.ends_with(".js");
            has_stylesheet |= path.starts_with("assets/") && path.ends_with(".css");
        }
    }
    assert!(
        has_script,
        "packaged frontend is missing its built JavaScript"
    );
    assert!(has_stylesheet, "packaged frontend is missing its built CSS");
}
