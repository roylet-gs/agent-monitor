# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.43"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.43/agent-monitor-v2.1.43-darwin-arm64.tar.gz"
      sha256 "21e87e40bdff8f65e208dc0452d5552c87bcdb53a01d59c9085058c7703e7cf2"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.43/agent-monitor-v2.1.43-darwin-x64.tar.gz"
      sha256 "2b3f6eb9ab670dca8b12ee7f19053d78a724d0804fc3c50442f97b3dc2dea1f7"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.43/agent-monitor-v2.1.43-linux-arm64.tar.gz"
      sha256 "65bb6ef07e6c1d27ee7e29b057f6f7052f171f60688705d7537bd7c0bdf7e63d"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.43/agent-monitor-v2.1.43-linux-x64.tar.gz"
      sha256 "87a0ad1eb0c14b3b466d2c3dec01a9f838af27391b128135c4a90aedea7f96cb"
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
