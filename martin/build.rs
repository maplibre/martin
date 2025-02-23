#[cfg(feature = "webui")]
/// copies a directory and its contents to a new location recursively
fn copy_dir_all(
    src: impl AsRef<std::path::Path>,
    dst: impl AsRef<std::path::Path>,
    exclude_dirs: &[std::path::PathBuf],
) -> std::io::Result<()> {
    assert!(!exclude_dirs.contains(&src.as_ref().to_path_buf()));
    assert!(src.as_ref().is_dir(), "source for the copy operation is not an existing directory");
    assert!(!dst.as_ref().exists(), "destination for the copy operation must not exist");
    std::fs::create_dir_all(&dst)?;
    
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.as_ref().join(entry.file_name());
        if ty.is_dir() {
            if exclude_dirs.contains(&src_path) {
                continue;
            }
            copy_dir_all(src_path, &dst_path, exclude_dirs)?;
        } else {
            std::fs::copy(src_path, dst_path)?;
        }
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    #[cfg(feature = "webui")]
    {
        // rust requires that all changes are done in OUT_DIR.
        //
        // We thus need to
        // - move the frontend code to the OUT_DIR,
        // - install npm dependencies and
        // - build the frontend
        let martin_ui_dir = std::path::PathBuf::from("martin-ui");
        assert!(martin_ui_dir.is_dir(), "martin-ui directory does not exist");
        let out_dir = std::env::var("OUT_DIR")
            .unwrap()
            .parse::<std::path::PathBuf>()
            .unwrap();
        let out_martin_ui_dir = out_dir.join("martin-ui");
        if out_martin_ui_dir.exists() {
            std::fs::remove_dir_all(&out_martin_ui_dir)?;
        }
        copy_dir_all(&martin_ui_dir, &out_martin_ui_dir, &[martin_ui_dir.join("dist"), martin_ui_dir.join("node_modules")])?;
        
        let target_to_keep = martin_ui_dir.join("dist");
        assert!(!target_to_keep.exists() || target_to_keep.is_dir(), "the martin-ui/dist must either not exist or have been produced by previous builds");

        println!("installing and building in {out_martin_ui_dir:?}");
        static_files::NpmBuild::new(&out_martin_ui_dir)
            .install()?
            .run("build")?
            .target(&target_to_keep)
            .to_resource_dir()
            .build()?;
        // Above code does have the problem that change detection would not be working properly.
        //
        // `copy_dir_all` was never anticipated by the crate we use
        // => we need to do this with different arguments.
        println!("success -> change_detection");
        static_files::NpmBuild::new(martin_ui_dir)
            .target(&target_to_keep)
            .change_detection();
    }
    Ok(())
}
