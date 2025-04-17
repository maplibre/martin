# Martins documentation

Martins documentation is available at <https://maplibre.org/martin>.

To build/develop this documentation locally, you can install `mdbook` and `mdbook-linkcheck`.
This can be done by running the following commands:

```bash
cargo install mdbook
```

## Development

You can simply edit the markdown files in the `src` directory and run the following command (from the project root directory) to preview the changes:

```bash
mdbook watch --open docs
```

Next to showing you how the docs will look, this also runs a link checker to ensure that all links are valid.

> [!TIP]
> Make sure that all pages are linked from [`src/SUMMARY.md`](src/SUMMARY.md).
> **Only** pages linked will be rendered.
> See the mdbook documentation for more information.

> [!NOTE]
> Files may only be added, not renamed.
> If you want to rename a file, make sure to [add a redirect to the new file in `output.html.redirect`](https://rust-lang.github.io/mdBook/format/configuration/renderers.html#outputhtmlredirect).
> Renaming files would otherwise break existing, external links for example on stackoverflow or github-discussions.
>
> Removing is bad for the same reasons and will be handled on a case-by-case basis.
