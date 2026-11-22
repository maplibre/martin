"""Turn an extracted `cargo package` directory into a library-only `martin-vendored-*` crate.

Usage: vendor-crate.py <package-dir> <dest-dir> <new-name> <origin> <keep,...> [<no-default-dep,...>]
"""

import re
import shutil
import sys
from pathlib import Path

src, dst, name, origin = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], sys.argv[4]
keep = sys.argv[5].split(",")
no_default = sys.argv[6].split(",") if len(sys.argv) > 6 and sys.argv[6] else []

if dst.exists():
    shutil.rmtree(dst)
dst.mkdir(parents=True)
for entry in keep:
    path = src / entry
    if path.is_dir():
        shutil.copytree(path, dst / entry)
    else:
        shutil.copy2(path, dst / entry)

manifest = (src / "Cargo.toml").read_text()
dropped = re.compile(r"^\[\[(test|bench|example)\]\]|^\[dev-dependencies|^\[profile")
sections = []
for section in re.split(r"(?m)^(?=\[)", manifest):
    if dropped.match(section):
        continue
    if section.startswith("[package]"):
        section = re.sub(r'(?m)^name = ".*"$', f'name = "{name}"', section)
        section = re.sub(
            r'(?m)^description = "(.*)"$',
            lambda m: f'description = "{m.group(1)} (vendored by Martin from {origin})"',
            section,
        )
    if section.startswith("[lib]"):
        section = section.rstrip("\n") + "\ntest = false\ndoctest = false\nbench = false\n\n"
    for dep in no_default:
        if section.startswith(f"[dependencies.{dep}]\n"):
            section = section.rstrip("\n") + "\ndefault-features = false\n\n"
    sections.append(section)

body = "".join(sections)
body = body[body.index("[package]") :].rstrip("\n")
header = f"# Vendored by Martin from {origin}. Regenerate with `just vendor-cql2` instead of editing.\n"
lints = '[lints.rust]\nwarnings = "allow"\n\n[lints.clippy]\nall = "allow"\n'
(dst / "Cargo.toml").write_text(f"{header}{body}\n\n{lints}")
