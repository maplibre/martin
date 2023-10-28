## Font Sources

Martin can serve font assests(`otf`, `ttf`, `ttc`) for map rendering, and there is no need to supply a large number of small pre-generated font protobuf files. Martin can generate them dynamically on the fly based on your request.   

## API
You can request font protobuf of single or combination of fonts.

||API|Demo|
|----|----|----|
|Single|/font/{fontstack}/{start}-{end}|http://127.0.0.1:3000/font/Overpass Mono Bold/0-255|
|Combination|/font/{fontstack1},{fontstack2},{fontstack_n}/{start}-{end}|http://127.0.0.1:3000/font/Overpass Mono Bold,Overpass Mono Light/0-255|

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

## Configuring from CLI
A font directory can be configured from the [CLI](run-with-cli.md) with the `--font` flag. The flag can be used multiple times to configure multiple font directories. 

```shell
martin --font /path/to/font_dir1 --font /path/to/font_dir2
```

## Configuring from Config File

A font directory can be configured from the config file with the `fonts` key.

```yaml
# Fonts configuration
fonts:
  - /path/to/fonts_dir1
  - /path/to/fonts_dir2
```