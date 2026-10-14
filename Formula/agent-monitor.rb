# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.24"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.24/agent-monitor-v2.1.24-darwin-arm64.tar.gz"
      sha256 "328d3ed7417d6d4d8951a1f6c9485b6d17cce194d8531be94be209f11daeacef"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.24/agent-monitor-v2.1.24-darwin-x64.tar.gz"
      sha256 "933128b3169f0faf570c8e6cbc9a0fb3b7537c4ccc4e475140cfd7c8f6f5f91e"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.24/agent-monitor-v2.1.24-linux-arm64.tar.gz"
      sha256 "17445b11e51baacfaf9174d13a78d1c4579d9f15de63156bfb41d62112733bc1"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.24/agent-monitor-v2.1.24-linux-x64.tar.gz"
      sha256 "2c2de0cdc165101d2f847856f6d7af6af8377bccfce8e2f42d9ea4b9ee1ef219"
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
