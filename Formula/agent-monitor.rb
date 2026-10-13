# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.18"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.18/agent-monitor-v2.1.18-darwin-arm64.tar.gz"
      sha256 "dea8d979ec65fd4dba0ba84b11437e13e72285b146d46130f5ee8ec9aea70852"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.18/agent-monitor-v2.1.18-darwin-x64.tar.gz"
      sha256 "7893e5039c5b279df491afb88654934999cf81c0efd4d12981bfc7fd24628702"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.18/agent-monitor-v2.1.18-linux-arm64.tar.gz"
      sha256 "f87c8193b6256b727bf884e6b1b6e130eb670013da4df5f8037737df67432389"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.18/agent-monitor-v2.1.18-linux-x64.tar.gz"
      sha256 "228965e2252c08283f97b31d68b52d938f819130418f67ca203e133ab92b0db5"
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
