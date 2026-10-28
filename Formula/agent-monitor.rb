# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.53"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.53/agent-monitor-v2.1.53-darwin-arm64.tar.gz"
      sha256 "befc6fb56a2bf9183b8f6a7c4b63ab99f0666a1e6218763a5420ab90dd4d6314"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.53/agent-monitor-v2.1.53-darwin-x64.tar.gz"
      sha256 "85bb9b357866f818c3b56e060a7b33c87b5602ff9bcc288f4bdfb367827764cb"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.53/agent-monitor-v2.1.53-linux-arm64.tar.gz"
      sha256 "081aa44f0a61fd022414d7685a05490f173f42fdda2c766b8260ff86d5828981"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.53/agent-monitor-v2.1.53-linux-x64.tar.gz"
      sha256 "43d3b2ad14a07dc1a7dba6adde2b52b5b0f4ec28c5b06c02b6e1a2be53d0f6d2"
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
