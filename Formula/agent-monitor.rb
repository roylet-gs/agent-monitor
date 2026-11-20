# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.87"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.87/agent-monitor-v2.1.87-darwin-arm64.tar.gz"
      sha256 "1a2ae7723447da5117f8d339c3789893bec57c9af822036af5aab56694f498aa"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.87/agent-monitor-v2.1.87-darwin-x64.tar.gz"
      sha256 "1d35d0ce2d941ff8315fd2857b2c5cc5e07d474471f136d5069215b8848c2c37"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.87/agent-monitor-v2.1.87-linux-arm64.tar.gz"
      sha256 "030b56144cbade630369f277822e5d725db0dfd116a873e32dccbff50fa26cc3"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.87/agent-monitor-v2.1.87-linux-x64.tar.gz"
      sha256 "dc1f5634508566c6ac7df7ecb0532ab50d452276d892c568ba3cc993344a9b08"
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
