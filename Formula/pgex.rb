class Pgex < Formula
  desc "Copy PostgreSQL tables with SQL row filters and consistent snapshots"
  homepage "https://github.com/pg-herd/pgex"
  url "https://github.com/pg-herd/pgex/releases/download/v0.1.1/pgex-v0.1.1-aarch64-apple-darwin.tar.gz"
  version "0.1.1"
  sha256 "b89dd321fce3316723a5d74245364f10e7f8be5042c86d7409e40baa74e0be92"
  license "MIT"

  bottle do
    root_url "https://github.com/pg-herd/pgex/releases/download/v0.1.1"
    sha256 cellar: :any, arm64_sequoia: "a29acd4b669a524321fde912253cdea6c3645ef0bdfe297c7940a6e3f3b499bb"
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
