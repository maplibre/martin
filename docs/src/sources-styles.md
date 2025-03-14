## Style Sources

Martin will serve your styles as needed by MapLibre.

To edit these styles, we recomend using <https://maputnik.github.io/editor/>.

### API

Martin can serve [MapLibre Style Spec](https://maplibre.org/maplibre-style-spec/).
Currently any static file can be used, but in the future, there will be additional optimisations resulting in usage restrictions.

You can use the `/catalog` api to see all the `<style_id>`s.

### Map Style

You can use the `/style/<style_id>` api to get `<style_id>`.

Changes or removals of styles are reflected immediately, but additions are not.
A restart of martin is required to see new styles.

### Add server-side raster tile rendering

This is not implemented yet, but there is a plan to add it.
Please see <https://github.com/maplibre/martin/issues/978> for more information.