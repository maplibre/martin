class Martin < Formula
  current_version="0.8.7"

  desc "Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support"
  homepage "https://github.com/maplibre/martin"
  url "https://github.com/maplibre/martin/releases/download/v#{current_version}/martin-Darwin-x86_64.tar.gz"

  # This is the sha256 checksum of the martin-Darwin-x86_64.tar.gz file
  # I am not certain if arch64 should have a different sha256 somewhere
  sha256 "92f660b1bef3a54dc84e4794a5ba02a8817c25f21ce7000783749bbae9e50de1"
  version "#{current_version}"

  depends_on "openssl@3"

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
