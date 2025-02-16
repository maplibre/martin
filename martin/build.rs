fn main() -> std::io::Result<()> {
    #[cfg(feature = "webui")]
    {
        static_files::NpmBuild::new("martin-ui")
            .install()?
            .run("build")?
            .target("martin-ui/dist")
            .change_detection()
            .to_resource_dir()
            .build()?;
    }
    Ok(())
}
