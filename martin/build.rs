#[cfg(feature = "webui")]
fn webui() {
    let martin_ui_dir = std::env::current_dir()
        .expect("Unable to get current dir")
        .join("martin-ui");
    assert!(martin_ui_dir.is_dir(), "martin-ui directory does not exist");
    let out_martin_ui_dir = std::env::var("OUT_DIR")
        .expect("OUT_DIR environment variable is not set")
        .parse::<std::path::PathBuf>()
        .expect("OUT_DIR environment variable is not a valid path")
        .join("martin-ui");

    println!("installing and building in {}", out_martin_ui_dir.display());
    let target_to_keep = martin_ui_dir.join("dist");
    assert!(
        !target_to_keep.exists() || target_to_keep.is_dir(),
        "the martin-ui/dist must either not exist or have been produced by previous builds"
    );

    static_files::NpmBuild::new(martin_ui_dir)
        .node_modules_strategy(static_files::NodeModulesStrategy::MoveToOutDir)
        .install()
        .expect("npm install failed")
        .run("build")
        .expect("npm run build failed")
        .target(target_to_keep)
        .change_detection()
        .to_resource_dir()
        .build()
        .expect("failed to build webui npm dir");
}

fn main() {
    #[cfg(feature = "webui")]
    if option_env!("RUSTDOC").is_none() {
        webui();
    }
}
