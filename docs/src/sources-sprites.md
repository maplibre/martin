## Sprite Sources

Given a directory with SVG images, Martin will generate a sprite -- a JSON index and a PNG image, for both low and highresolution displays. The SVG filenames without extension will be used as the sprites' image IDs (remember that one sprite and thus `sprite_id` contains multiple images).
The images are searched recursively in the given directory, so subdirectory names will be used as prefixes for the image IDs.
For example `icons/bicycle.svg` will be available as `icons/bicycle` sprite image.

The sprite generation is not yet cached, and may require external reverse proxy or CDN for faster operation.
If you would like to improve this, please drop us a pull request.

### API

Martin uses [MapLibre sprites API](https://maplibre.org/maplibre-style-spec/sprite/) specification to serve sprites via
several endpoints. The sprite image and index are generated on the fly, so if the sprite directory is updated, the
changes will be reflected immediately.

You can use the `/catalog` api to see all the `<sprite_id>`s with their contained sprites.

##### Sprite PNG

![sprite](sources-sprites.png)

`GET /sprite/<sprite_id>.png` endpoint contains a single PNG sprite image that combines all sources images.
Additionally, there is a high DPI version available at `GET /sprite/<sprite_id>@2x.png`.

##### Sprite index

`/sprite/<sprite_id>.json` metadata index describing the position and size of each image inside the sprite. Just like
the PNG, there is a high DPI version available at `/sprite/<sprite_id>@2x.json`.

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

Multiple `sprite_id` values can be combined into one sprite with the same pattern as for tile
joining:  `/sprite/<sprite_id1>,<sprite_id2>,...,<sprite_idN>`. No ID renaming is done, so identical sprite names will
override one another.

### Configuring from CLI

A sprite directory can be configured from the CLI with the `--sprite` flag. The flag can be used multiple times to
configure multiple sprite directories. The `sprite_id` of the sprite will be the name of the directory -- in the example below,
the sprites will be available at `/sprite/sprite_a` and `/sprite/sprite_b`. Use `--save-config` to save the
configuration to the config file.

```bash
martin --sprite /path/to/sprite_a --sprite /path/to/other/sprite_b
```

To use generate Signed Distance Fields (SDFs) images instead, the `--make_sdf` Option can be added.
SDF-Images allow their color to be set at runtime in the map redering engine.

```bash
martin --sprite /path/to/sprite_a --sprite  /path/to/other/sprite_b --make_sdf
```

### Configuring with Config File

A sprite directory can be configured from the config file with the `sprite` key, similar to
how [MBTiles and PMTiles](config-file.md) are configured.

```yaml
# Sprite configuration
sprites:
  paths:
    # all SVG files in this directory will be published under the sprite_id "my_images"
    - /path/to/my_images
  sources:
    # SVG images in this directory will be published under the sprite_id "my_sprites"
    my_sprites: /path/to/some_dir
  # This tells Martin to handle images in directories as Signed Distance Fields (SDFs)
  # SDF-Images allow their color to be set at runtime in the map redering engine.
  # Defaults to `false`.
  make_sdf: false
```

The sprites are now avaliable at `/sprite/my_images,some_dir.png`/ ...
