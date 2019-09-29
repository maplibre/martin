pub const USAGE: &str = "
Martin - PostGIS Mapbox Vector Tiles server.

Usage:
  martin [options] [<connection>]
  martin -h | --help
  martin -v | --version

Options:
  -h --help               Show this screen.
  -v --version            Show version.
  --config=<path>         Path to config file.
  --keep-alive=<n>        Connection keep alive timeout [default: 75].
  --listen-addresses=<n>  The socket address to bind [default: 0.0.0.0:3000].
  --pool-size=<n>         Maximum connections pool size [default: 20].
  --watch                 Scan for new sources on sources list requests
  --workers=<n>           Number of web server workers.
";

#[derive(Debug, Deserialize)]
pub struct Args {
  pub arg_connection: Option<String>,
  pub flag_config: Option<String>,
  pub flag_help: bool,
  pub flag_keep_alive: Option<usize>,
  pub flag_listen_addresses: Option<String>,
  pub flag_pool_size: Option<u32>,
  pub flag_watch: bool,
  pub flag_version: bool,
  pub flag_workers: Option<usize>,
}
