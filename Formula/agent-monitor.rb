# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.2"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.2/agent-monitor-v2.1.2-darwin-arm64.tar.gz"
      sha256 "b998db3926d24115eefd1a81e9d382f7e3339421b10d13f6f9ff53ecdf8a0221"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.2/agent-monitor-v2.1.2-darwin-x64.tar.gz"
      sha256 "d0bc893d67193ed8a7d607b684eca874810eeaa35074d538a9012710e880a7f0"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.2/agent-monitor-v2.1.2-linux-arm64.tar.gz"
      sha256 "08131ab35620f6ef284a3687d3d2eab9f44ab028de25b99550001f53604f9a47"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.2/agent-monitor-v2.1.2-linux-x64.tar.gz"
      sha256 "7b3e9d38f0130dfcc15e8c0e4b688d511e707c2d215f907798018c9783353b67"
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
