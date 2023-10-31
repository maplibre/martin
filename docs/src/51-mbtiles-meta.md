# `mbtiles` Metadata Access

## meta-all
Print all metadata values to stdout, as well as the results of tile detection. The format of the values printed is not stable, and should only be used for visual inspection.

```shell
mbtiles meta-all my_file.mbtiles
```

## meta-get
Retrieve raw metadata value by its name. The value is printed to stdout without any modifications.  For example, to get the `description` value from an mbtiles file:

```shell
mbtiles meta-get my_file.mbtiles description
```

## meta-set
Set metadata value by its name, or delete the key if no value is supplied. For example, to set the `description` value to `A vector tile dataset`:

```shell
mbtiles meta-set my_file.mbtiles description "A vector tile dataset"
```
