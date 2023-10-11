class Martin < Formula
  current_version="0.9.0"

  desc "Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support"
  homepage "https://github.com/maplibre/martin"

  on_arm do
    sha256 "d1a64d4707e3f1fdb41b3e445c462465e6150d3b30f7520af262548184a4d08b"
    url "https://github.com/maplibre/martin/releases/download/v#{current_version}/martin-Darwin-aarch64.tar.gz"
  end
  on_intel do
    sha256 "ab581373a9fe699ba8e6640b587669391c6d5901ce816c3acb154f8410775068"
    url "https://github.com/maplibre/martin/releases/download/v#{current_version}/martin-Darwin-x86_64.tar.gz"
  end
  
  version "#{current_version}"

  def install
    bin.install "martin"
  end

  def caveats; <<~EOS
    Martin requires a database connection string.
    It can be passed as a command-line argument or as a DATABASE_URL environment variable.
      martin postgres://postgres@localhost/db
  EOS
  end

  test do
    `#{bin}/martin --version`
  end
end
