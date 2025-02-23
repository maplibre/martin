/// copies a directory and its contents to a new location recursively
fn copy_dir_all(
    src: impl AsRef<std::path::Path>,
    dst: impl AsRef<std::path::Path>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(&dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
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
        let out_dir = std::env::var("OUT_DIR").unwrap();
        let new_dir = format!("{out_dir}/martin-ui/");
        copy_dir_all("martin-ui", &new_dir)?;

        static_files::NpmBuild::new(&new_dir)
            .install()?
            .run("build")?
            .target("martin-ui/dist")
            .change_detection()
            .to_resource_dir()
            .build()?;
        // Above code does have the problem that change detection would not be working properly.
        //
        // `copy_dir_all` was never anticipated by the crate we use
        // => we need to do this with different arguments.
        static_files::NpmBuild::new("martin-ui")
            .target(&out_dir)
            .change_detection();
    }
    Ok(())
}
