#!/usr/bin/env python3
"""Download toner/positron style JSONs and fonts; rewrite styles (oberbayern, MLT, martin glyphs)."""
import json
import urllib.request

STYLES = (
    ("toner", "https://raw.githubusercontent.com/openmaptiles/maptiler-toner-gl-style/master/style.json"),
    ("positron", "https://raw.githubusercontent.com/openmaptiles/positron-gl-style/master/style.json"),
)

# Google Fonts (raw GitHub) — Noto Sans + Nunito for Toner/Positron styles
# Nunito: repo has variable font Nunito[wght].ttf; we use it for Regular (no static/ in ofl/nunito)
FONTS = (
    ("NotoSans-Regular.ttf", "https://github.com/google/fonts/raw/main/ofl/notosans/NotoSans-Regular.ttf"),
    ("NotoSans-Bold.ttf", "https://github.com/google/fonts/raw/main/ofl/notosans/NotoSans-Bold.ttf"),
    ("NotoSans-Italic.ttf", "https://github.com/google/fonts/raw/main/ofl/notosans/NotoSans-Italic.ttf"),
    ("Nunito-Regular.ttf", "https://github.com/google/fonts/raw/main/ofl/nunito/Nunito%5Bwght%5D.ttf"),
)

STYLES_DIR = "/out/styles"
FONTS_DIR = "/out/fonts"


def download(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "Martin-demo-build/1.0"})
    with urllib.request.urlopen(req) as r:
        return r.read()


def main():
    for name, url in STYLES:
        path = f"{STYLES_DIR}/{name}.json"
        d = json.loads(download(url).decode())
        d["sources"]["openmaptiles"]["url"] = "../oberbayern"
        d["sources"]["openmaptiles"]["encoding"] = "mlt"
        d["glyphs"] = "../font/{fontstack}/{range}"
        with open(path, "w") as f:
            json.dump(d, f, indent=1)

    for filename, url in FONTS:
        path = f"{FONTS_DIR}/{filename}"
        try:
            data = download(url)
            with open(path, "wb") as f:
                f.write(data)
        except Exception as e:
            print(f"Warning: could not download {filename}: {e}")


if __name__ == "__main__":
    main()
