# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.49"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.49/agent-monitor-v2.1.49-darwin-arm64.tar.gz"
      sha256 "6fbabefa6818c0c3d34e6cbf445bd83364c89f20d49a19086c563b0eba1dbd7f"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.49/agent-monitor-v2.1.49-darwin-x64.tar.gz"
      sha256 "14a119d4b0d30e1c2f9d7829584da3e7921b1abfacf1aeb3ea61d6aa3da22b00"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.49/agent-monitor-v2.1.49-linux-arm64.tar.gz"
      sha256 "05880f59b7078460a05835dedb543713cce0b00604d241200112cb09fc70a634"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.49/agent-monitor-v2.1.49-linux-x64.tar.gz"
      sha256 "c68278d4d4cdb43b1cb281c2a81f9baa217811795c6da7df35af3e5140e5f145"
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
