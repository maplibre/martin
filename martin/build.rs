#[cfg(feature = "webui")]
fn main() -> std::io::Result<()> {
    static_files::NpmBuild::new("../martin-ui")
        .install()?
        .run("build")?
        .target("../martin-ui/dist")
        .change_detection()
        .to_resource_dir()
        .build()
}

#[cfg(not(feature = "webui"))]
fn main() {}
