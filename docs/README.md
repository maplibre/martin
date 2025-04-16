# Martins documentation

Martins documentation is available at <https://maplibre.org/martin>.

To build/develop this documentation locally, you can install `mdbook` and `mdbook-linkcheck`.
This can be done by running the following commands:

```bash
cargo install mdbook
cargo install mdbook-linkcheck
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
