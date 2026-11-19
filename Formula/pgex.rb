class Pgex < Formula
  desc "Copy PostgreSQL tables with SQL row filters and consistent snapshots"
  homepage "https://github.com/pg-herd/pgex"
  url "https://github.com/pg-herd/pgex/releases/download/v0.1.2/pgex-v0.1.2-aarch64-apple-darwin.tar.gz"
  version "0.1.2"
  sha256 "3fcdd677e1d372714302de6f243b55ee9f122b1512c2e488da34f5985d0df033"
  license "MIT"

  bottle do
    root_url "https://github.com/pg-herd/pgex/releases/download/v0.1.2"
    sha256 cellar: :any, arm64_sequoia: "ece892a2b142bddf9e03c392298894ba9cd97fc21d5f59b1b9757a184a69f1f9"
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
