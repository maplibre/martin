#[cfg(feature = "webui")]
fn main() -> std::io::Result<()> {
    static_files::NpmBuild::new("../ui")
        .install()?
        .run("build")?
        .target("../ui/dist")
        .change_detection()
        .to_resource_dir()
        .build()
}

#[cfg(not(feature = "webui"))]
fn main() {}
