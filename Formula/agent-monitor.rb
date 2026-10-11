# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.0.0"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.0.0/agent-monitor-v2.0.0-darwin-arm64.tar.gz"
      sha256 "e0ebfd67814c3f1b3dc48754be7ba1d962cea19c3437bccfa17fcdc28fc305d1"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.0.0/agent-monitor-v2.0.0-darwin-x64.tar.gz"
      sha256 "18d799f6339f52150efbc83991f0c3838d282341ef7b6edf60877f98d028299d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.0.0/agent-monitor-v2.0.0-linux-arm64.tar.gz"
      sha256 "6c97d364221fabaf9d440f4e9225ca3a3cafa7861e3d41b937dcb62aee938cd5"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.0.0/agent-monitor-v2.0.0-linux-x64.tar.gz"
      sha256 "b6a3df92662486e6f2d4270e54759fc825f7f7a1d15ed6b77c0f5826f0bd193f"
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
