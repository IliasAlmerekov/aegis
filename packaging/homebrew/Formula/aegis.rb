class Aegis < Formula
  desc "Heuristic shell guardrail for AI agent command execution"
  homepage "https://github.com/IliasAlmerekov/aegis"
  version "0.5.6"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/IliasAlmerekov/aegis/releases/download/v0.5.6/aegis-macos-aarch64"
      sha256 "8768865f2456c115788967ab29db790106e8a87fea8aa654561f8e942ec172b0"
    else
      url "https://github.com/IliasAlmerekov/aegis/releases/download/v0.5.6/aegis-macos-x86_64"
      sha256 "092d91e7b22800e68df8290707031beac531feb44bd02ddb0356ff082299ff2e"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/IliasAlmerekov/aegis/releases/download/v0.5.6/aegis-linux-aarch64"
      sha256 "5e900632230750dff271d5816bd7c81a53158005d88c30ec439e59df871d0e31"
    else
      url "https://github.com/IliasAlmerekov/aegis/releases/download/v0.5.6/aegis-linux-x86_64"
      sha256 "3af320804df191d3a10637e3da103dc306a5d573a85f1d740726d62c0826f683"
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
