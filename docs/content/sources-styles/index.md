---
description: Serving MapLibre style JSON files
icon: material/palette
tags:
  - styles
  - configuration
---

# Style Sources

Martin will serve your styles as needed by MapLibre rendering libraries.

To edit a style, open the Styles tab in the Martin web UI and select Edit; the editor shows the style's layers on the left, the live map in the middle, and an inspector on the right with Layer, Palette, and Checks tabs.
Every property editor follows the MapLibre style spec (v8) and accepts fixed values, zoom-dependent values, values by category of a feature attribute (`match`), values by a numeric attribute, or raw expressions, with attribute names and example values taken from the tile source.
The Palette tab lists every color the style uses, and changing one updates every layer that uses it; the Checks tab reports labels with low contrast and style spec errors.
Edits stay in your browser until you click Download, which saves the style JSON; replace the file Martin serves with it to publish your changes, since Martin does not write style files.
"Open in Maputnik" remains available from the editor header.

### API

Martin can serve [MapLibre Style Spec](https://maplibre.org/maplibre-style-spec/).
Currently, Martin will use any valid [`JSON`](https://json.org) file as a style,
but in the future, we may optimize Martin which may result in additional restrictions.

Use the `/catalog` API to see all the `<style_id>`s.

### Map Style

Use the `/style/<style_id>` API to get a `<style_id>`'s JSON content.

Changes or removals of styles are reflected immediately, but additions are not.
A restart of Martin is required to see new styles.

### Configuring with Config File

Styles are configured with the `styles` key.
`paths` lists files or directories, and every `.json` file found is published under its file name.
`sources` maps a `<style_id>` to one file.
`collections` lists directories of per-project directories, and a style at `/projects/styles/project1/basic.json` is published as `project1.basic`.

```yaml
styles:
  paths:
    - /path/to/style.json
    - /path/to/style_dir
  sources:
    my_style: /path/to/another_style.json
  collections:
    - /projects/styles
```

### Server-side raster tile rendering

Martin can also render a style server-side into raster images -
both as XYZ tiles and as a single static image at a chosen camera, with an
optional GeoJSON overlay.

See [Server-side raster tile rendering](rendering.md) for how to
enable it, the endpoints, and the static-image overlay API.
