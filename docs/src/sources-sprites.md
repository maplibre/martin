## Sprite Sources

Given a directory with SVG images, Martin will generate a sprite -- a JSON index and a PNG image, for both low and high resolution displays. The SVG filenames without extension will be used as the sprite image IDs. The images are searched recursively in the given directory, so subdirectory names will be used as prefixes for the image IDs, e.g. `icons/bicycle.svg` will be available as `icons/bicycle` sprite image. The sprite generation is not yet cached, and may require external reverse proxy or CDN for faster operation.

### API

Martin uses [MapLibre sprites API](https://maplibre.org/maplibre-style-spec/sprite/) specification to serve sprites via several endpoints. The sprite image and index are generated on the fly, so if the sprite directory is updated, the changes will be reflected immediately.

##### Sprite PNG

![sprite](sources-sprites.png)

`GET /sprite/<sprite_id>.png` endpoint contains a single PNG sprite image that combines all sources images. Additionally, there is a high DPI version available at `GET /sprite/<sprite_id>@2x.png`.

##### Sprite index

`/sprite/<sprite_id>.json` metadata index describing the position and size of each image inside the sprite. Just like the PNG, there is a high DPI version available at `/sprite/<sprite_id>@2x.json`.

```json
{
  "bicycle": {
    "height": 15,
    "pixelRatio": 1,
    "width": 15,
    "x": 20,
    "y": 16
  },
  ...
}
```

#### Combining Multiple Sprites

Multiple sprite_id values can be combined into one sprite with the same pattern as for tile joining:  `/sprite/<sprite_id1>,<sprite_id2>,...,<sprite_idN>`. No ID renaming is done, so identical sprite names will override one another.

### Configuring from CLI

A sprite directory can be configured from the CLI with the `--sprite` flag. The flag can be used multiple times to configure multiple sprite directories. The name of the sprite will be the name of the directory -- in the example below, the sprites will be available at `/sprite/sprite_a` and `/sprite/sprite_b`.  Use `--save-config` to save the configuration to the config file.

```shell
martin --sprite /path/to/sprite_a --sprite /path/to/other/sprite_b
```

### Configuring with Config File

A sprite directory can be configured from the config file with the `sprite` key, similar to how [MBTiles and PMTiles](config-file.md) are configured.

```yaml
# Sprite configuration
sprites:
  paths:
    # all SVG files in this dir will be published as a "my_images" sprite source
    - /path/to/my_images
  sources:
    # SVG images in this directory will be published as a "my_sprites" sprite source
    my_sprites: /path/to/some_dir
```
