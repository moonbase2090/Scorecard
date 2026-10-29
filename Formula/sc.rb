class Sc < Formula
  desc "Code quality and test coverage analysis for software repositories"
  homepage "https://github.com/moonbase2090/Scorecard"
  license any_of: ["Apache-2.0", "MIT"]

  on_macos do
    on_arm do
      url "https://github.com/moonbase2090/Scorecard/releases/download/v0.1.14/sc-v0.1.14-aarch64-apple-darwin.tar.gz"
      sha256 "c10691fc29f5d68d2ccfa7d3ca492b2389c600370fe650459ab6c29f9f7dbd14"
    end
    on_intel do
      url "https://github.com/moonbase2090/Scorecard/releases/download/v0.1.14/sc-v0.1.14-x86_64-apple-darwin.tar.gz"
      sha256 "46a3b5a48283a96726dbd3395be9870325caf033ffeb9444c23b22932bf67c36"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/moonbase2090/Scorecard/releases/download/v0.1.14/sc-v0.1.14-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "a7fb9419e0b65aaff10bf37fae0fdd166b285b1370e3ab12c3437f016c409912"
    end
    on_intel do
      url "https://github.com/moonbase2090/Scorecard/releases/download/v0.1.14/sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "107b49f0c643db045df2349c37429c2e150b6b274b1eb992660031e221747b1b"
    end
  end

  def install
    bin.install "sc", "sc-mcp"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/sc --version")
    assert_match version.to_s, shell_output("#{bin}/sc-mcp --version")
  end
end
