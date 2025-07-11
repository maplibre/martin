## Style Sources

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

### Server-side raster tile rendering

TODO, document
