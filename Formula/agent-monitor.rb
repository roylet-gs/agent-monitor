# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.83"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.83/agent-monitor-v2.1.83-darwin-arm64.tar.gz"
      sha256 "7d40f825900b289ec853fb2857baa264b883a7637f7280a35e7dcc4d1ca4b5a0"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.83/agent-monitor-v2.1.83-darwin-x64.tar.gz"
      sha256 "56eb43cc4873d775660a7c92315e99b71685266d1281c97e1dc0088bc6caa895"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.83/agent-monitor-v2.1.83-linux-arm64.tar.gz"
      sha256 "2e76feddd2c813fd6950b281c9131ac0df2930d5dfaf594ddd034a3c56d53e86"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.83/agent-monitor-v2.1.83-linux-x64.tar.gz"
      sha256 "a6c16c4f2c03bd77a43cdef71d2646258dea28572b742641379d7df71fa78aab"
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
