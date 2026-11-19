class Pgex < Formula
  desc "Copy PostgreSQL tables with SQL row filters and consistent snapshots"
  homepage "https://github.com/pg-herd/pgex"
  url "https://github.com/pg-herd/pgex/releases/download/v0.1.5/pgex-v0.1.5-aarch64-apple-darwin.tar.gz"
  version "0.1.5"
  sha256 "3429da959a999159979ffb5e92a2cf699445e42d28c1d2e030ea2fa6a9cda26f"
  license "MIT"

  bottle do
    root_url "https://github.com/pg-herd/pgex/releases/download/v0.1.5"
    sha256 cellar: :any, arm64_sequoia: "bde0b28a0f145f4485a86c56c75e2f343b654edb33398ed89d1239057f8c8064"
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
    assert_match "Usage: pgex [OPTIONS]", shell_output("#{bin}/pgex --help")
  end
end
