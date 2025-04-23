# Martin as a library

Martin can be used as a standalone server, or as a library in your own Rust application. When used as a library, you can use the following features:

* `webui` - enable web UI
* tile sources
  * `mbtiles` - enable MBTile tile sources
  * `pmtiles` - enable PMTile tile sources
  * `postgres` - enable PostgreSQL/PostGIS tile sources
* supporting resources
  * `fonts` - enable font sources
  * `sprites` - enable sprite sources
  * `styles` - enable style sources
* `lambda` - add specialised support for running in serverless functions

If you are missing a part of Martin functionality in the [public `martin` API](https://docs.rs/martin), we would love to hear from you.
Please open an issue on our GitHub repository or directly open a pull request.
