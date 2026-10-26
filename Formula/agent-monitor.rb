# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.45"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.45/agent-monitor-v2.1.45-darwin-arm64.tar.gz"
      sha256 "09e035b829c6158a568f6b18205ed242ba813c32bef43ff55f51a7c25164fe8f"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.45/agent-monitor-v2.1.45-darwin-x64.tar.gz"
      sha256 "5edb463cd8c544aac97363665e85e3187bf899b877fcbed9925cb5ed80e4c489"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.45/agent-monitor-v2.1.45-linux-arm64.tar.gz"
      sha256 "9c39d460ae9903982a6176efe6d4d25085698098648edd217f34242df783a582"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.45/agent-monitor-v2.1.45-linux-x64.tar.gz"
      sha256 "b51a2760345c6f69ba2508bfcb1c44472a21f039e1f5fbecdafcd912cdf1acb9"
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
