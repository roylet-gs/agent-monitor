# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.63"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.63/agent-monitor-v2.1.63-darwin-arm64.tar.gz"
      sha256 "34037e67df2ca62de8258dc52919787a907a45524fbd3477d308501036ec8b5e"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.63/agent-monitor-v2.1.63-darwin-x64.tar.gz"
      sha256 "aaa9efacf398e4354f7c2a55a31781dc4cea11b1585c7271249b59155aad2d53"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.63/agent-monitor-v2.1.63-linux-arm64.tar.gz"
      sha256 "aa9aa816e8cfdf5a205d68aa1926f2af7cf1aa46427246f7d94443eb3df66b9a"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.63/agent-monitor-v2.1.63-linux-x64.tar.gz"
      sha256 "0984ca21f113265a50d110455ec9d86bdd07b43308a17d4e80d3f439f9926c1f"
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
