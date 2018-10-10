pub const USAGE: &str = "
Martin - PostGIS Mapbox Vector Tiles server.

Usage:
  martin [options] [<connection>]
  martin -h | --help
  martin -v | --version

Options:
  -h --help               Show this screen.
  -v --version            Show version.
  --workers=<n>           Number of web server workers.
  --pool_size=<n>         Maximum connections pool size [default: 20].
  --keep_alive=<n>        Connection keep alive timeout [default: 75].
  --listen_addresses=<n>  The socket address to bind [default: 0.0.0.0:3000].
  --config=<path>         Path to config file.
";

#[derive(Debug, Deserialize)]
pub struct Args {
  pub flag_help: bool,
  pub flag_version: bool,
  pub flag_workers: Option<usize>,
  pub flag_pool_size: Option<u32>,
  pub flag_keep_alive: Option<usize>,
  pub flag_listen_addresses: Option<String>,
  pub flag_config: Option<String>,
  pub arg_connection: Option<String>,
}
