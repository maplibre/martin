---
icon: material/palette
tags:
  - styles
  - configuration
---

# Style Sources

Martin will serve your styles as needed by MapLibre rendering libraries.

To edit these styles, we recommend using <https://maputnik.github.io/editor/>.

### API

Martin can serve [MapLibre Style Spec](https://maplibre.org/maplibre-style-spec/).
Currently, Martin will use any valid [`JSON`](https://json.org) file as a style,
but in the future, we may optimise Martin which may result in additional restrictions.

Use the `/catalog` API to see all the `<style_id>`s.

### Map Style

Use the `/style/<style_id>` API to get a `<style_id>`'s JSON content.

Changes or removals of styles are reflected immediately, but additions are not.
A restart of Martin is required to see new styles.

#### Merged styles

Request up to 128 comma-separated IDs to combine styles into one document:

```text
/style/<style1>,<style2>,…,<styleN>
```

The `.json` suffix remains optional. Martin appends layers in request order, so
layers from later styles render above layers from earlier styles. Root settings
such as camera, projection, terrain, light, and metadata come from the first
style. The `font-faces` and `state` maps are merged by key so layers retain
their font and global-state dependencies. Identical entries are de-duplicated.
The same key with different definitions returns `400 Bad Request`.
the same key with different definitions returns `400 Bad Request`.

Sources with identical complete definitions are de-duplicated. If the same
definition has different names, layers are rewritten to use the first name. A
shared source name with different definitions, or a duplicate layer ID, returns
`400 Bad Request` rather than silently renaming application-visible IDs.

All non-empty `glyphs` values must use the same URL template. Identical sprite
URLs are de-duplicated, multiple Martin `/sprite/<id>` URLs are combined through
the composite sprite endpoint only when they point to the Martin instance
serving the style, and multiple-sprite arrays are merged by sprite ID. Distinct
external sprite URLs and other conflicting sprite definitions return
`400 Bad Request`. Composite sprites retain Martin's existing behavior when two
sprite sources contain the same image name. No automatic image renaming is
performed.

Tile sources remain separate in the merged style. Server-side raster and static
rendering continue to accept one style ID only.

### Server-side raster tile rendering

On Linux, Martin can also render a style server-side into raster images -
both as XYZ tiles and as a single static image at a chosen camera, with an
optional GeoJSON overlay.

See [Server-side raster tile rendering](rendering.md) for how to
enable it, the endpoints, and the static-image overlay API.
