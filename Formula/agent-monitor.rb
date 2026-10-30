# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.76"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.76/agent-monitor-v2.1.76-darwin-arm64.tar.gz"
      sha256 "e70c1305adfb439bbd699f8b13957c737a8f8d3faca76db0182caf1930c5535f"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.76/agent-monitor-v2.1.76-darwin-x64.tar.gz"
      sha256 "57eb0f96f5edcf57fafb5780fd10bca60a42dab45f509a489e687d89867b8b3d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.76/agent-monitor-v2.1.76-linux-arm64.tar.gz"
      sha256 "8cde504f5e1ed064e1718f30ac138c7c0f0eff8af9903d0ac1ea34c6e3e45bb8"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.76/agent-monitor-v2.1.76-linux-x64.tar.gz"
      sha256 "bc2bf2c6ba93169d904bd184a869acd62f50ddcef11e644f655bc89672bba483"
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
