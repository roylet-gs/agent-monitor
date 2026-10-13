# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.7"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.7/agent-monitor-v2.1.7-darwin-arm64.tar.gz"
      sha256 "fe6c58eac3b1fa4a11c0bf060fca275a1167b2834265cb4501a21a54780c7486"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.7/agent-monitor-v2.1.7-darwin-x64.tar.gz"
      sha256 "2e5d2a0b25c0e163fef01ab772c4d1b9cb934cd49e2360d944273e0c8ed51209"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.7/agent-monitor-v2.1.7-linux-arm64.tar.gz"
      sha256 "8bed14a0c1b95d4cad92c7536a1ccb87b9d7fc1e1efa70fde746c3628e32cd79"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.7/agent-monitor-v2.1.7-linux-x64.tar.gz"
      sha256 "7445d1b30e2fb0d1eefd0421bfa2fa7c30beb6199cf7391b4bdab7d8175dd37e"
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
