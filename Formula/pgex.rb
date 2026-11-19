class Pgex < Formula
  desc "Copy PostgreSQL tables with SQL row filters and consistent snapshots"
  homepage "https://github.com/pg-herd/pgex"
  url "https://github.com/pg-herd/pgex/releases/download/v0.1.3/pgex-v0.1.3-aarch64-apple-darwin.tar.gz"
  version "0.1.3"
  sha256 "d1854b3d50238ecbb0492c50a1db65ee447d399d28f63a95e520920477065356"
  license "MIT"

  bottle do
    root_url "https://github.com/pg-herd/pgex/releases/download/v0.1.3"
    sha256 cellar: :any, arm64_sequoia: "31e450beb581cf5db808254a7d40e50621e7e5ef3ee5eb679e3a05fb5994f1f4"
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
