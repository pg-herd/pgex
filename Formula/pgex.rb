class Pgex < Formula
  desc "Copy PostgreSQL tables with SQL row filters and consistent snapshots"
  homepage "https://github.com/pg-herd/pgex"
  url "https://github.com/pg-herd/pgex/releases/download/v0.1.4/pgex-v0.1.4-aarch64-apple-darwin.tar.gz"
  version "0.1.4"
  sha256 "8ab12304c668c3def9c9637a82bc8ccc3b8775cea4fcc34f05e3a5955a9aa2fe"
  license "MIT"

  bottle do
    root_url "https://github.com/pg-herd/pgex/releases/download/v0.1.4"
    sha256 cellar: :any, arm64_sequoia: "a5c7924579b6a40c51217dc7f9c1240c3af0e64c3fafe2de833115450554d5f4"
  end

  depends_on arch: :arm64
  depends_on macos: :sequoia
  depends_on "libpq"

  def install
    bin.install "pgex"
    doc.install "README.md", "CONTRIBUTING.md"
    pkgshare.install "licenses"
  end

  test do
    assert_match "Refresh tables in a PostgreSQL destination", shell_output("#{bin}/pgex --help")
  end
end
