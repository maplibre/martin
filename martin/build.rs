#[cfg(feature = "webui")]
use std::fs;
#[cfg(feature = "webui")]
use std::path::Path;

#[cfg(feature = "webui")]
fn copy_file_tree(src: &Path, dst: &Path, exclude_dirs: &[&str]) {
    assert!(
        src.is_dir(),
        "source for the copy operation is not an existing directory"
    );
    let _ = fs::remove_dir_all(dst); // ignore if dir does not exist
    fs::create_dir_all(dst).unwrap_or_else(|e| {
        panic!(
            "failed to create destination directory {}: {e}",
            dst.display()
        )
    });
    let excludes = exclude_dirs.iter().map(|v| src.join(v)).collect::<Vec<_>>();

    let mut it = walkdir::WalkDir::new(src).follow_links(true).into_iter();
    while let Some(entry) = it.next() {
        let entry = entry.expect("failed to read directory entry");
        if excludes.iter().any(|v| v == entry.path()) {
            it.skip_current_dir();
            continue;
        }

        // Get the relative path of the entry
        let dst_path = dst.join(
            entry
                .path()
                .strip_prefix(src)
                .expect("path is not a prefix of the source directory"),
        );

        if entry.file_type().is_dir() {
            fs::create_dir_all(&dst_path).unwrap_or_else(|e| {
                panic!(
                    "failed to create destination directory {}: {e}",
                    dst_path.display()
                )
            });
        } else {
            fs::copy(entry.path(), &dst_path).unwrap_or_else(|e| {
                panic!(
                    "failed to copy file {} to {}: {e}",
                    entry.path().display(),
                    dst_path.display()
                )
            });
        }
    }
}

#[cfg(feature = "webui")]
fn webui() {
    // rust requires that all changes are done in OUT_DIR.
    //
    // We thus need to
    // - move the frontend code to the OUT_DIR
    // - install npm dependencies
    // - build the frontend
    let martin_ui_dir = std::env::current_dir()
        .expect("Unable to get current dir")
        .join("martin-ui");
    assert!(martin_ui_dir.is_dir(), "martin-ui directory does not exist");

    let out_martin_ui_dir = std::env::var("OUT_DIR")
        .expect("OUT_DIR environment variable is not set")
        .parse::<std::path::PathBuf>()
        .expect("OUT_DIR environment variable is not a valid path")
        .join("martin-ui");

    copy_file_tree(&martin_ui_dir, &out_martin_ui_dir, &[
        "dist",
        "node_modules",
    ]);

    println!("installing and building in {out_martin_ui_dir:?}");
    static_files::NpmBuild::new(&out_martin_ui_dir)
        .install()
        .expect("npm install failed")
        .run("build")
        .expect("npm run build failed")
        .target(out_martin_ui_dir.join("dist"))
        .to_resource_dir()
        .build()
        .expect("failed to build webui npm dir");

    let target_to_keep = martin_ui_dir.join("dist");
    assert!(
        !target_to_keep.exists() || target_to_keep.is_dir(),
        "the martin-ui/dist must either not exist or have been produced by previous builds"
    );

    // TODO: we may need to move index.html one level down per change_detection() docs
    static_files::NpmBuild::new(martin_ui_dir)
        .target(&target_to_keep)
        .change_detection();
}

fn main() {
    #[cfg(feature = "webui")]
    webui();
}
