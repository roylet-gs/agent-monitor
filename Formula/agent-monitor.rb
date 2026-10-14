# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.29"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.29/agent-monitor-v2.1.29-darwin-arm64.tar.gz"
      sha256 "f1d4cb6b598e0974689de62e9f4c8b472aee1b8dd34888a0de76d14f79189b69"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.29/agent-monitor-v2.1.29-darwin-x64.tar.gz"
      sha256 "132ad820d25c12dc7578b4a499bbd46b4c6885165ec791892cacda5f56dcdbd8"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.29/agent-monitor-v2.1.29-linux-arm64.tar.gz"
      sha256 "d08ad63d2cdf8acc6ac52ade7ca0ffdf07da852cbf0d2fe86ca1fbae1afa3997"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.29/agent-monitor-v2.1.29-linux-x64.tar.gz"
      sha256 "f32e9972834ed47f5029d3098b8ca7560b69178a194d33e53e372184a4bd72cc"
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
