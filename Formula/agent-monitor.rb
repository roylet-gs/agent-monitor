# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.71"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.71/agent-monitor-v2.1.71-darwin-arm64.tar.gz"
      sha256 "d8b122e763ecf1d6f1d50ab1be55eb04923f46f889ca4e1e57f855812798288e"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.71/agent-monitor-v2.1.71-darwin-x64.tar.gz"
      sha256 "6d449f14f07259eabec8037c68f1ce8d87ba9677984c9d14dd3c87612b546182"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.71/agent-monitor-v2.1.71-linux-arm64.tar.gz"
      sha256 "8d317539c575ae47480a9bfc003bfa525c0961c13146c030763f7b0a7a16464a"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.71/agent-monitor-v2.1.71-linux-x64.tar.gz"
      sha256 "786e0428115177e290999d414f1c5a559bdeb84764195a3c19f2df04e1963d6f"
    end
  end

  def install
    bin.install "am", "am-tui", "am-hook"
  end

  def caveats
    "Run `am hooks install` once so Claude Code reports agent status to am."
  end

  test do
    assert_match "am-hook", shell_output("#{bin}/am-hook --version")
  end
end
