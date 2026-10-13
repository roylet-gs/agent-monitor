# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.5"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.5/agent-monitor-v2.1.5-darwin-arm64.tar.gz"
      sha256 "85f699047ac9e37dc781402a6a77e0e2708b0df3cece283eeab412fe4d3b6b07"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.5/agent-monitor-v2.1.5-darwin-x64.tar.gz"
      sha256 "2cb66ff4c85b0daa30bf8ecc2835f13289e90d67906835d816cdd243e7069f6a"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.5/agent-monitor-v2.1.5-linux-arm64.tar.gz"
      sha256 "515af5f1defbb98bbd964eacd9d13c793cf8ccdaa9f7d82d495aa8dfb39d345a"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.5/agent-monitor-v2.1.5-linux-x64.tar.gz"
      sha256 "3ea8bb9c2a4016d4f0202739c9ce15d1155eae9d269ee8ce9ba861fdc60fd36e"
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
