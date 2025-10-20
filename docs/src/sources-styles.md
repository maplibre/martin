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

> [!NOTE]
> This feature is currently marked as unstable/experimental and thus not included in the default build.
> The behaviour on this endpoint may change in patch releases.
>
> To enable it, build Martin with the `--features=unstable-rendering` flag after installing the nessesary dependencys via `just install-dependencies`.
>
> It is experimental due to the limitations of our current implementation:
> - Rendering support is currently only available on Linux.
>   To add support for macOS/Windows, please see <https://github.com/maplibre/maplibre-native-rs>.
> - Currently, martin does not cache style rendered requests and
> - does not support concurrency for this feature.


We support generating a rasterised image for an XYZ tile of a given style.
Use the `/style/<style_id>/{z}/{x}/{y}.{filetype}` API to get a `<style_id>`'s rendered png/jpeg content.

### Static image prepraration

We currently do not have the same [capabilites as Tileserver-GL](https://tileserver.readthedocs.io/en/latest/endpoints.html#static-images) to layout images.
We are working on adding this feature and are very open to contributions.
