# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.79"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.79/agent-monitor-v2.1.79-darwin-arm64.tar.gz"
      sha256 "91681c7b15a3078d79311e13c708297d848259fd5a698eaa79c5103a17794bdd"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.79/agent-monitor-v2.1.79-darwin-x64.tar.gz"
      sha256 "964594b94f3a3054710ac8cf384468eb71c903f79377ee4e136c6587adb98658"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.79/agent-monitor-v2.1.79-linux-arm64.tar.gz"
      sha256 "ef85df3f2be962c8bf16141f44158c02d4ce32f3dd626c35e8f62e00883e059d"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.79/agent-monitor-v2.1.79-linux-x64.tar.gz"
      sha256 "325ad45902a21294189c2a9bdee7e3436457168390d6a652d7245f6dafc0d755"
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
