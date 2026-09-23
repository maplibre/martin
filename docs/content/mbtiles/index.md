---
tags:
  - mbtiles
  - tools
---

# Working with MBTiles archives

Martin includes `mbtiles` utility to interact with the [`*.mbtiles` files](../mbtiles-schema.md) from the command line. It allows users to [examine](../mbtiles-meta.md), [copy](../mbtiles-copy.md), [validate](../mbtiles-validation.md) or [compare and apply diffs between them](../mbtiles-diff.md).

This tool can be installed by compiling the latest released version with `cargo install mbtiles --locked`, or by downloading a pre-built binary from the [releases page](https://github.com/maplibre/martin/releases/latest).

Use `mbtiles --help` to see a list of available commands:

```text
--8<-- "help/mbtiles.txt"
```

And `mbtiles <command> --help` to see help for a specific command. Example for `mbtiles validate --help`:

```text
--8<-- "help/mbtiles-validate.txt"
```

## Temporary files

If `mbtiles copy` or `mbtiles validate` fails with `database or disk is full` on a large archive, SQLite's temporary files may have filled a different partition. On Unix-like systems, set `SQLITE_TMPDIR` to an existing directory with write and execute permissions and enough free space:

```bash
SQLITE_TMPDIR=/data/tmp mbtiles copy src.mbtiles dst.mbtiles
```

By default, SQLite checks `SQLITE_TMPDIR`, `TMPDIR`, `/var/tmp`, `/usr/tmp`, `/tmp`, then the current directory, using the first accessible directory. See [SQLite's temporary file locations](https://www.sqlite.org/tempfiles.html#temporary_file_storage_locations).
