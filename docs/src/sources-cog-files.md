# Cloud Optimized GeoTIFF File Sources

Martin can serve local [COG(Cloud Optimized GeoTIFF)](https://cogeo.org/) files. For cog on remote like S3, you could track it on [issue 875](https://github.com/maplibre/martin/issues/875), we are working on and welcome any assistance.

> A Cloud Optimized GeoTIFF (COG) is a regular GeoTIFF file, aimed at being hosted on a HTTP file server, with an internal organization that enables more efficient workflows on the cloud. It does this by leveraging the ability of clients issuing ​HTTP GET range requests to ask for just the parts of a file they need.

|colory type|bits per sample|supported|status|
|----|----|----|----|
|rgb/rgba|8|✅||
|rgb/rgba|16/32...|🛠️|working on|
|gray|8/16/32...|🛠️|working on|

## Run Martin with CLI to serve cog fiels

```bash
martin /path/to/dir_contains_cog /path/to/cog.tif
```

## Run Martin with configuration file

```yml
keep_alive: 75

# The socket address to bind [default: 0.0.0.0:3000]
listen_addresses: '0.0.0.0:3000'

# Number of web server workers
worker_processes: 8

# Amount of memory (in MB) to use for caching tiles [default: 512, 0 to disable]
cache_size_mb: 8

# Database configuration. This can also be a list of PG configs.

cog:
  paths:
    # scan this whole dir, matching all *.tif files
    - /dir-path
    # specific tif file will be published as a cog source
    - /path/to/pmt.pmtiles
  sources:
    # named source matching source name to a single file
     cog-src1: /path/to/cog1.tif
     cog-src2: /path/to/cog2.tif
```
