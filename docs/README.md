# Martins documentation

The documentation of `maplibre/martin` is available at <https://maplibre.org/martin>.

To build/develop this documentation locally, you can run the following command from the root of the project.
This will install zensical as a docker container and build and serve the documentation:

```bash
just docs
```

> [!NOTE]
> You will need docker installed on your system.

The configuration for the documentation lives in `zensical.toml`.

## Development

You can simply edit the markdown files in the `src` directory and run `just docs` (from the project root directory) to preview the changes.

Next to showing you how the docs will look, this also runs a link checker to ensure that all links are valid.

> [!TIP]
> Make sure that all pages are linked from the nav section in [`zensical.toml`](../zensical.toml).
> **Only** pages linked will be rendered.
> See the [zensical documentation](https://zensical.org/docs/setup/navigation/) for more information.

> [!NOTE]
> Files may only be added, not renamed.
> If you want to rename a file, make sure to [add a redirect to the new file in `output.html.redirect`](https://rust-lang.github.io/mdBook/format/configuration/renderers.html#outputhtmlredirect).
> Renaming files would otherwise break existing, external links for example on stackoverflow or github-discussions.
>
> Removing is bad for the same reasons and will be handled on a case-by-case basis.
