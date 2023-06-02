# Tools

## Mbtiles tools:
A small binary utility that allows users to interact with mbtiles files from the CLI as follows: `mbtiles <command> <file.mbtiles>`
- ### `meta-get`:
    Retrieve a metadata value by key: `mbtiles meta-get <file.mbtiles> <options> <key>`.  
    #### Options:
  - `-r, --raw` : return the raw metadata value