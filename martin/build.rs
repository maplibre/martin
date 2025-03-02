#[cfg(feature = "webui")]
/// copies a directory and its contents to a new location recursively
fn copy_dir_all(
    src: &std::path::PathBuf,
    dst: &std::path::PathBuf,
    exclude_dirs: &[std::path::PathBuf],
) -> std::io::Result<()> {
    assert!(!exclude_dirs.contains(src) && !exclude_dirs.contains(dst));
    assert!(src.is_dir());

    // creating symlinks is cheap => recreate instead of sync
    if dst.exists() {
        std::fs::remove_dir_all(dst)?;
    }
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if exclude_dirs.contains(&entry.path()) {
            continue;
        }
        let target = dst.join(entry.file_name());
        #[cfg(unix)]
        std::os::unix::fs::symlink(entry.path(), target)?;
        #[cfg(windows)]
        if entry.file_type()?.is_dir() {
            std::os::windows::fs::symlink_dir(entry.path(), target)
        } else {
            std::os::windows::fs::symlink_file(entry.path(), target)
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
        copy_dir_all(
            &martin_ui_dir,
            &out_martin_ui_dir,
            &[
                martin_ui_dir.join("dist"),
                martin_ui_dir.join("node_modules"),
            ],
        )?;

        let target_to_keep = martin_ui_dir.join("dist");
        assert!(
            !target_to_keep.exists() || target_to_keep.is_dir(),
            "the martin-ui/dist must either not exist or have been produced by previous builds"
        );

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
