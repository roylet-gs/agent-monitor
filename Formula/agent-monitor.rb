# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.78"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.78/agent-monitor-v2.1.78-darwin-arm64.tar.gz"
      sha256 "ce407bf39e354e82172ccadffa6f259a2d102f232cf67b38a406e424235b1f93"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.78/agent-monitor-v2.1.78-darwin-x64.tar.gz"
      sha256 "e6c25a8ce69d138d5c212f6cd25037bbfafd83012e2e1ff3ea4e434313bd090a"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.78/agent-monitor-v2.1.78-linux-arm64.tar.gz"
      sha256 "96a0b0266eafd492d1d212dfe432efc4334de075a09b8eb611b40da01eeb6ac1"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.78/agent-monitor-v2.1.78-linux-x64.tar.gz"
      sha256 "5a71f9d42cdbfa56cb5ac46c1639387f0a2314effbb4a004d26a2e312650119f"
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
