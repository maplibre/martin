# Working with MBTiles archives

Martin includes `mbtiles` utility to interact with the [`*.mbtiles` files](<https://maplibre.org/martin/mbtiles-schema/index.md>) from the command line. It allows users to [examine](<https://maplibre.org/martin/mbtiles-meta/index.md>), [copy](<https://maplibre.org/martin/mbtiles-copy/index.md>), [validate](<https://maplibre.org/martin/mbtiles-validation/index.md>) or [compare and apply diffs between them](<https://maplibre.org/martin/mbtiles-diff/index.md>).

This tool can be installed by compiling the latest released version with `cargo install mbtiles --locked`, or by downloading a pre-built binary from the [releases page](<https://github.com/maplibre/martin/releases/latest>).

Use `mbtiles --help` to see a list of available commands:

```text
A utility to work with .mbtiles file content

Usage: mbtiles <COMMAND>

Commands:
  summary      Show MBTiles file summary statistics
  meta-all     Prints all values in the metadata table in a free-style, unstable YAML format
  meta-get     Gets a single value from the MBTiles metadata table
  meta-set     Sets a single value in the MBTiles metadata table or deletes it if no value
  diff         Compare two files A and B, and generate a new diff file. If the diff file is applied to A, it will produce B
  copy         Copy tiles from one mbtiles file to another
  apply-patch  Apply diff file generated from 'copy' command
  meta-update  Update metadata to match the content of the file
  validate     Validate tile data if hash of tile data exists in file
  pack         Pack a directory tree of tiles into an MBTiles file
  cache-purge  Remove expired entries from a tile-cache MBTiles file (see the cache schema), and optionally evict entries to bound the file size
  unpack       Unpack an MBTiles file into a directory tree of tiles
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=mbtiles=debug. See https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html for more information.
```

And `mbtiles <command> --help` to see help for a specific command. Example for `mbtiles validate --help`:

```text
Validate tile data if hash of tile data exists in file

Usage: mbtiles validate [OPTIONS] <FILE>

Arguments:
  <FILE>
          MBTiles file to validate

Options:
      --integrity-check <INTEGRITY_CHECK>
          Value to specify the extent of the SQLite integrity check performed

          [default: quick]
          [possible values: quick, full, off]

      --agg-hash <AGG_HASH>
          How should the aggregate tiles hash be checked or updated

          Possible values:
          - verify: Verify that the aggregate tiles hash value in the metadata table matches the computed value. Used by default
          - update: Update the aggregate tiles hash value in the metadata table
          - off:    Do not check the aggregate tiles hash value

  -h, --help
          Print help (see a summary with '-h')
```

## Temporary files

If `mbtiles copy` or `mbtiles validate` fails with `database or disk is full` on a large archive, SQLite's temporary files may have filled a different partition. On Unix-like systems, set `SQLITE_TMPDIR` to an existing directory with write and execute permissions and enough free space:

```bash
SQLITE_TMPDIR=/data/tmp mbtiles copy src.mbtiles dst.mbtiles
```

By default, SQLite checks `SQLITE_TMPDIR`, `TMPDIR`, `/var/tmp`, `/usr/tmp`, `/tmp`, then the current directory, using the first accessible directory. See [SQLite's temporary file locations](<https://www.sqlite.org/tempfiles.html#temporary_file_storage_locations>).
