class Aegis < Formula
  desc "Heuristic shell guardrail for AI agent command execution"
  homepage "https://github.com/IliasAlmerekov/aegis"
  version "0.5.6"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/IliasAlmerekov/aegis/releases/download/v0.5.6/aegis-macos-aarch64"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    else
      url "https://github.com/IliasAlmerekov/aegis/releases/download/v0.5.6/aegis-macos-x86_64"
      sha256 "1111111111111111111111111111111111111111111111111111111111111111"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/IliasAlmerekov/aegis/releases/download/v0.5.6/aegis-linux-aarch64"
      sha256 "2222222222222222222222222222222222222222222222222222222222222222"
    else
      url "https://github.com/IliasAlmerekov/aegis/releases/download/v0.5.6/aegis-linux-x86_64"
      sha256 "3333333333333333333333333333333333333333333333333333333333333333"
    end
  end

  def install
    bin.install Dir["aegis-*"].first => "aegis"
  end

  def caveats
    <<~EOS
      Homebrew installs the aegis binary only.

      To install supported Claude Code and Codex hooks after installation:
        aegis install-hooks --all

      To use Aegis as a shell proxy, configure your shell or agent explicitly:
        export SHELL="$(brew --prefix)/bin/aegis"
        export AEGIS_REAL_SHELL="/path/to/your/real/shell"

      Native Windows shells are not supported; use Aegis from WSL2 on Windows.
    EOS
  end

  test do
    assert_match "brew-test", shell_output("#{bin}/aegis -c 'echo brew-test'")
  end
end