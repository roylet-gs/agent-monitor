# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.16"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.16/agent-monitor-v2.1.16-darwin-arm64.tar.gz"
      sha256 "1b4a12a7b044ecabe454625d3e42024f9b725e63354258e0fde55b32e031edd4"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.16/agent-monitor-v2.1.16-darwin-x64.tar.gz"
      sha256 "a49b4b0d32e724f32b24d51f194c26064b2cc5c251ff9a6fad8f953c649c5405"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.16/agent-monitor-v2.1.16-linux-arm64.tar.gz"
      sha256 "80c59599325f0c01be78e76a7ef0701034aad9e33846ca4953df67150f87f3d0"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.16/agent-monitor-v2.1.16-linux-x64.tar.gz"
      sha256 "61f865c9b07e472300dc6d561d8fabf6e82edacf00171d3e2b515e93dbfeae49"
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
