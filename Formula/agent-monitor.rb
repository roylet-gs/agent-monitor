# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.65"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.65/agent-monitor-v2.1.65-darwin-arm64.tar.gz"
      sha256 "88f3b0c1036bcc10ac4c773307da3cd0abe332396612f361cbeb5ea5bfb3384f"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.65/agent-monitor-v2.1.65-darwin-x64.tar.gz"
      sha256 "6b1e20b8d60bc11c1a0c535702f2e2333b34d46210dba2d702c76bfa3f26ebe0"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.65/agent-monitor-v2.1.65-linux-arm64.tar.gz"
      sha256 "2df895c00835d4b70a66573ac1b7b88f503048a7d401f23ea19c6029d85cc900"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.65/agent-monitor-v2.1.65-linux-x64.tar.gz"
      sha256 "ef2e6a8e80295bb063d85d84c6f4cb39ca1c56c3232c7085d352bd0b1694ad8f"
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
