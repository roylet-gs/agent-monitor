# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.21"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.21/agent-monitor-v2.1.21-darwin-arm64.tar.gz"
      sha256 "3552865f301d727c7a29f76d99d7fea83fd7975b9bfe2b4b4e2c93772c387064"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.21/agent-monitor-v2.1.21-darwin-x64.tar.gz"
      sha256 "d0e3f41f000574d95ae361d8958d439833d2bab9bf684c7dd062571e340d1715"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.21/agent-monitor-v2.1.21-linux-arm64.tar.gz"
      sha256 "0e2a04b662ff2ea17ee4915af321943a4d983562dc679d7112a3494156fa8135"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.21/agent-monitor-v2.1.21-linux-x64.tar.gz"
      sha256 "19c529d80a8e53964eec2f3499438502d80f42667160b037c6f848955e7915f9"
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
