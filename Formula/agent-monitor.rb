# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.85"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.85/agent-monitor-v2.1.85-darwin-arm64.tar.gz"
      sha256 "4af0569c266b180b2ce91d5320bf483014fa140c7ce0cdbd325b4af6f9885171"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.85/agent-monitor-v2.1.85-darwin-x64.tar.gz"
      sha256 "4a45b2fccfba0fd76043ea516ca5a1fd544a8ccc00e9b9756549cf1070486e82"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.85/agent-monitor-v2.1.85-linux-arm64.tar.gz"
      sha256 "4cad09d5deac7686f85515037fa53dbb03dda5b6d639abca64bece666ac0aca8"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.85/agent-monitor-v2.1.85-linux-x64.tar.gz"
      sha256 "bf09f05289bf463e0d8301dd3856479059e2b55d71cd72c9a6b9f13465652951"
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
