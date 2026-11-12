# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.84"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.84/agent-monitor-v2.1.84-darwin-arm64.tar.gz"
      sha256 "bc0b62dc2f789bb0dd99168193e856b3d868ff92ea05717ff212c8225b0bcddd"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.84/agent-monitor-v2.1.84-darwin-x64.tar.gz"
      sha256 "55a54e292f90c3d653c9a1b4d1ce38d134be486a7c1ac60535289686ff69dfb0"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.84/agent-monitor-v2.1.84-linux-arm64.tar.gz"
      sha256 "c5fec850fa5d967018ad49b8d6b2a28e764f5b6d30ac30b2eea8eeae37752bbc"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.84/agent-monitor-v2.1.84-linux-x64.tar.gz"
      sha256 "f5440cbdad8b37153515e64df0509ff6e400cac5c79557e12a42dfaa87900e16"
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
