class Martin < Formula
  current_version="0.9.1"

  desc "Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support, plus an mbtiles tool"
  homepage "https://github.com/maplibre/martin"

  on_arm do
    sha256 "00828eb3490664eba767323da98d2847238b65b7bea1e235267e43a67277d8e5"
    url "https://github.com/maplibre/martin/releases/download/v#{current_version}/martin-Darwin-aarch64.tar.gz"
  end
  on_intel do
    sha256 "75b52bd89ba397267080e938dd261f57c1eabdaa1d27ac13bf4904031672a6e9"
    url "https://github.com/maplibre/martin/releases/download/v#{current_version}/martin-Darwin-x86_64.tar.gz"
  end
  
  version "#{current_version}"

  def install
    bin.install "martin"
    bin.install "mbtiles"
  end

  def caveats; <<~EOS
    Martin requires a database connection string.
    It can be passed as a command-line argument or as a DATABASE_URL environment variable.
      martin postgres://postgres@localhost/db
  EOS
  end

  test do
    `#{bin}/martin --version`
    `#{bin}/mbtiles --version`
  end
end
