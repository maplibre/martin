#!/usr/bin/env python3
"""Screenshot the major Martin UI views (needs `vite` on :3001 and martin on :3000).

Usage: python3 scripts/screenshots.py [OUT_DIR] [BASE_URL]
"""
import pathlib
import sys

from playwright.sync_api import TimeoutError as PlaywrightTimeoutError
from playwright.sync_api import sync_playwright

out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/martin-shots")
base = sys.argv[2] if len(sys.argv) > 2 else "http://localhost:3001/"
out.mkdir(parents=True, exist_ok=True)
for f in out.glob("*.png"):
    f.unlink()

VIEWS = [
    ("tiles", "?tab=tiles"),
    ("tiles-search", "?tab=tiles&search=geography"),
    ("tiles-inspect", "?tab=tiles&inspect=world_cities#map=2.4/25/115"),
    ("styles", "?tab=styles"),
    ("styles-guide", "?tab=styles&guide=maplibre_demo"),
    ("style-editor", "?style=maplibre_demo"),
    ("fonts", "?tab=fonts"),
    ("tiles-focus", "?tab=tiles"),
    ("sprites", "?tab=sprites"),
    ("sprites-preview", "?tab=sprites&preview=sprites"),
    ("sprites-download", "?tab=sprites&download=sprites"),
]
VIEWPORTS = {"desktop": (1440, 900), "mobile": (390, 844)}
THEMES = ["light", "dark"]

with sync_playwright() as p:
    browser = p.chromium.launch(args=[
            "--use-gl=swiftshader",
            "--enable-webgl",
            "--ignore-gpu-blocklist",
            # Maputnik (https) fetches the style from localhost; headless cannot answer the permission prompt
            "--disable-features=LocalNetworkAccessChecks,PrivateNetworkAccessChecks",
        ])
    for theme in THEMES:
        for vp, (w, h) in VIEWPORTS.items():
            if vp == "mobile" and theme == "dark":
                continue
            ctx = browser.new_context(viewport={"width": w, "height": h}, color_scheme=theme)
            ctx.add_init_script(f"localStorage.setItem('theme', '{theme}')")
            page = ctx.new_page()
            # Maputnik asks to confirm loading a style from a URL
            page.on("dialog", lambda d: d.accept())
            for name, query in VIEWS:
                page.goto(base + query)
                try:
                    page.wait_for_load_state("networkidle", timeout=8000)
                except PlaywrightTimeoutError:
                    pass  # polling/tile requests may keep the network busy
                if name.endswith("-focus"):
                    for _ in range(6):
                        page.keyboard.press("Tab")
                page.wait_for_timeout(14000 if "editor" in name else 3500 if "inspect" in name or "preview" in name else 1800)
                page.screenshot(path=str(out / f"{name}.{vp}.{theme}.png"), full_page=name.startswith(("tiles", "styles", "fonts", "sprites")) and "-" not in name or name == "tiles-search")
                print("shot", name, vp, theme)
                problems = page.evaluate(
                    """() => ({
                        broken: [...document.images].filter(i => i.complete && i.naturalWidth === 0).map(i => i.src.slice(0, 80)),
                        overflow: document.documentElement.scrollWidth > window.innerWidth,
                    })"""
                )
                if problems["broken"]:
                    print("  WARNING broken <img>:", problems["broken"])
                if problems["overflow"] and vp == "mobile":
                    print("  WARNING horizontal overflow on mobile")
            ctx.close()
    browser.close()
