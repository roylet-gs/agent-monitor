# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.81"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.81/agent-monitor-v2.1.81-darwin-arm64.tar.gz"
      sha256 "59b8429377e5040eb7547689e5375539572c3b09e98530604faafe067af68110"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.81/agent-monitor-v2.1.81-darwin-x64.tar.gz"
      sha256 "5bde17a5d2cfba956bc9f77febf8437860709028350fabb86869749c568f0eb2"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.81/agent-monitor-v2.1.81-linux-arm64.tar.gz"
      sha256 "96394fa0d24ca823186e213e2ae219346d91cf43a7633822bc2b3a54a3ba9e1b"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.81/agent-monitor-v2.1.81-linux-x64.tar.gz"
      sha256 "3e0ba0811e366d0517d2555893fcff3249cf9dbdf76675399d0e911cb9f03697"
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
