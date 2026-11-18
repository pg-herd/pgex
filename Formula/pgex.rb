class Pgex < Formula
  desc "Copy PostgreSQL tables with SQL row filters and consistent snapshots"
  homepage "https://github.com/pg-herd/pgex"
  url "https://github.com/pg-herd/pgex/releases/download/v0.1.0/pgex-v0.1.0-aarch64-apple-darwin.tar.gz"
  version "0.1.0"
  sha256 "e599eecdbc39e63604715d402594c5dda64d1758183c9a9c0f4cf742797ef8d8"
  license "MIT"

  bottle do
    root_url "https://github.com/pg-herd/pgex/releases/download/v0.1.0"
    sha256 cellar: :any, arm64_sequoia: "c09cee76ed511742933c713eda4aebc89f5601c072791f694e05eee861b89f1a"
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
