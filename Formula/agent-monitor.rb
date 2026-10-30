# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.75"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.75/agent-monitor-v2.1.75-darwin-arm64.tar.gz"
      sha256 "d6107b21e8fefaccea71b5cff5cf8df24a9aaa4a2d0c84b9f27079fb8e2d3d9d"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.75/agent-monitor-v2.1.75-darwin-x64.tar.gz"
      sha256 "04f2c24d14aeb3ed2eeb51cef55a07f32e73bdf140c45cbbd9e866e1fa190e48"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.75/agent-monitor-v2.1.75-linux-arm64.tar.gz"
      sha256 "185b580a0526385e38b20650476ac72a4971f8aab5a874e1ada7f227d0dd6fd0"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.75/agent-monitor-v2.1.75-linux-x64.tar.gz"
      sha256 "28f904bc6c93713dbdd4d0070a17f86a4316e880de578f24e3e99091d9ca33f8"
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
