class Protein < Formula
  desc "Terminal UI to build, launch, track and kill macOS caffeinate sessions"
  homepage "https://github.com/vianch/protein"
  url "https://github.com/vianch/protein/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "f14386cea94d67b5011c72983095466164edd156fb50b9093d78d2c0ddb209a8"
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
