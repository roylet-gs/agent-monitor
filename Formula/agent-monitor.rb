# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.61"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.61/agent-monitor-v2.1.61-darwin-arm64.tar.gz"
      sha256 "062e1edb3c4403a70f7be8862dfaddeb314d7e03962cc120fa80d2ea85997f80"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.61/agent-monitor-v2.1.61-darwin-x64.tar.gz"
      sha256 "5778705985e7985f43345692846e21965cc1be9d68b7eeb3df43c09dbaee2f2f"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.61/agent-monitor-v2.1.61-linux-arm64.tar.gz"
      sha256 "a7439ec701fb9a64cfa1e5587edfed6755ee3fa254e2864a21860e8e2af51f0f"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.61/agent-monitor-v2.1.61-linux-x64.tar.gz"
      sha256 "e74d5672b19b0e37620adcc923933f00bf5c7b03a2c7f11f0f22c65195d41bb4"
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
