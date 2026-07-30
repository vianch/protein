class Protein < Formula
  desc "Terminal UI to build, launch, track and kill macOS caffeinate sessions"
  homepage "https://github.com/vianch/protein"
  url "https://github.com/vianch/protein/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "3a6850b14d5855fe7b37d1860577a66882b6442ce11db841508f8f1e31b599a6"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
    bin.install_symlink bin/"caf" => "protein"
  end

  test do
    assert_match "caf", shell_output("#{bin}/protein --version")
  end
end
