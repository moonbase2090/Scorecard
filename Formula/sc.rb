class Sc < Formula
  desc "Code quality and test coverage analysis for software repositories"
  homepage "https://github.com/moonbase2090/Scorecard"
  license any_of: ["Apache-2.0", "MIT"]

  on_macos do
    on_arm do
      url "https://github.com/moonbase2090/Scorecard/releases/download/v0.1.13/sc-v0.1.13-aarch64-apple-darwin.tar.gz"
      sha256 "847ce1aa952e6f174997777d41df4f79d13b836ac29c880fe46bc07c469b997d"
    end
    on_intel do
      url "https://github.com/moonbase2090/Scorecard/releases/download/v0.1.13/sc-v0.1.13-x86_64-apple-darwin.tar.gz"
      sha256 "a92837b8ea3c6cc1b24d6d08f3ba7a57ea737e21d4aaedd91522c30cbddcbdca"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/moonbase2090/Scorecard/releases/download/v0.1.13/sc-v0.1.13-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "97c5e1366a90d0024524ceaa154324fa9307a7a7dd92c4d6105fd054e9d45854"
    end
    on_intel do
      url "https://github.com/moonbase2090/Scorecard/releases/download/v0.1.13/sc-v0.1.13-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "7ce440b67934e5baefa2b7bc7deebae107de7c088c78bb8c495091b5d5f298e9"
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
