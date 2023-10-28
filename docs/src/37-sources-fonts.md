## Font Sources

Martin can serve glyph ranges from `otf`, `ttf`, and `ttc` fonts as needed by MapLibre text rendering. Martin will generate them dynamically on the fly.
The glyph range generation is not yet cached, and may require external reverse proxy or CDN for faster operation.    

## API
Fonts ranges are available either for a single font, or a combination of multiple fonts. The font names are case-sensitive and should match the font name in the font file as published in the catalog. When combining multiple fonts, the glyph range will contain glyphs from the first listed font if available, and fallback to the next font if the glyph is not available in the first font, etc. The glyph range will be empty if none of the fonts contain the glyph.

| Type     | API                                            | Example                                                               |
|----------|------------------------------------------------|-----------------------------------------------------------------------|
| Single   | `/font/{name}/{start}-{end}`                   | `/font/Overpass Mono Bold/0-255`                    |
| Combined | `/font/{name1},{name2},{name_n}/{start}-{end}` | `/font/Overpass Mono Bold,Overpass Mono Light/0-255` |

Martin will list all the font resources in the `/catalog` endpoint, you could call it to check all your font resources before an accurate request.

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
A font file or directory can be configured from the [CLI](21-run-with-cli.md) with the `--font` flag. The flag can be used multiple times.

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
