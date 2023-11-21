## Font Sources

Martin can serve glyph ranges from `otf`, `ttf`, and `ttc` fonts as needed by MapLibre text rendering. Martin will generate them dynamically on the fly.
The glyph range generation is not yet cached, and may require external reverse proxy or CDN for faster operation.

## API
Fonts ranges are available either for a single font, or a combination of multiple fonts. The font names are case-sensitive and should match the font name in the font file as published in the catalog. Make sure to URL-escape font names as they usually contain spaces.

|         | Font Request                         |
|---------|--------------------------------------|
| Pattern | `/font/{name}/{start}-{end}`         |
| Example | `/font/Overpass%20Mono%20Bold/0-255` |


### Composite Font Request

When combining multiple fonts, the glyph range will contain glyphs from the first listed font if available, and fallback to the next font if the glyph is not available in the first font, etc. The glyph range will be empty if none of the fonts contain the glyph.

|         | Composite Font Request with fallbacks                        |
|---------|--------------------------------------------------------------|
| Pattern | `/font/{name1},…,{nameN}/{start}-{end}`                      |
| Example | `/font/Overpass%20Mono%20Bold,Overpass%20Mono%20Light/0-255` |

### Catalog
Martin will show all available fonts at the `/catalog` endpoint.

```shell
curl http://127.0.0.1:3000/catalog
{
  "fonts": {
    "Overpass Mono Bold": {
      "family": "Overpass Mono",
      "style": "Bold",
      "glyphs": 931,
      "start": 0,
      "end": 64258
    },
    "Overpass Mono Light": {
      "family": "Overpass Mono",
      "style": "Light",
      "glyphs": 931,
      "start": 0,
      "end": 64258
    },
    "Overpass Mono SemiBold": {
      "family": "Overpass Mono",
      "style": "SemiBold",
      "glyphs": 931,
      "start": 0,
      "end": 64258
    }
  }
}
```

## Using from CLI

A font file or directory can be configured from the [CLI](run-with-cli) with one or more `--font` parameters.

```shell
martin --font /path/to/font/file.ttf --font /path/to/font_dir
```

## Configuring from Config File

A font directory can be configured from the config file with the `fonts` key.

```yaml
# Fonts configuration
fonts:
  # A list of *.otf, *.ttf, and *.ttc font files and dirs to search recursively.
  - /path/to/font/file.ttf
  - /path/to/font_dir
```
