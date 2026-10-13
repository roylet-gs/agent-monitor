# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.3"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.3/agent-monitor-v2.1.3-darwin-arm64.tar.gz"
      sha256 "16f45aa383bd5347fab22fbac94681645f37ac545a93e8f987513536c8bb9cf5"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.3/agent-monitor-v2.1.3-darwin-x64.tar.gz"
      sha256 "5190a551d3ba1ade9e1a42a68e9bbfff94bc93030046255eed72eb921617dc33"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.3/agent-monitor-v2.1.3-linux-arm64.tar.gz"
      sha256 "4d037dff33876129343c05b19bf3f4a879dbb57bf1660fef39e1340bb6f3a5b4"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.3/agent-monitor-v2.1.3-linux-x64.tar.gz"
      sha256 "64159cf494f9015eb22261239f0236bc3276185fa43c058ed6b25bc75abbb8d2"
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
