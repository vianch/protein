class Proteine < Formula
  desc "Terminal UI to build, launch, track and kill macOS caffeinate sessions"
  homepage "https://github.com/vianch/protein"
  url "https://github.com/vianch/protein/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "2038aafe57a8b79d949c71719a9a745c217063b150b8ca536f3c82ad00a2f65e"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
    bin.install_symlink bin/"caf" => "proteine"
  end

  test do
    assert_match "caf", shell_output("#{bin}/proteine --version")
  end
end
