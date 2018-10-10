class Martin < Formula
  current_version="0.1.0"
  desc "PostGIS Mapbox Vector Tiles server"
  homepage "https://github.com/urbica/martin"
  url "https://github.com/urbica/martin/releases/download/v#{current_version}/martin-darwin-x86_64.zip"
  sha256 "fc7dac7da8d2773ab033091b8c3321271c77e2a54014a09ae0de284089c02763"

  def install
    bin.install "martin"
  end

  test do
    system "false"
    # `#{bin}/martin --version`
  end
end