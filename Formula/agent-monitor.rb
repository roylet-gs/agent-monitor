# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.69"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.69/agent-monitor-v2.1.69-darwin-arm64.tar.gz"
      sha256 "bf8d5971db599b146acc1cb71bddac1c04fea5cf96572248b43f3f137cb3af35"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.69/agent-monitor-v2.1.69-darwin-x64.tar.gz"
      sha256 "e0f6ba7244d34b72e0f2ca1de975adba66716474cd576d08db0c61b2db9af1f0"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.69/agent-monitor-v2.1.69-linux-arm64.tar.gz"
      sha256 "13c4c011990e40344d06bdd3a95f8d0017183f9252dec427ee11e4619818f4a7"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.69/agent-monitor-v2.1.69-linux-x64.tar.gz"
      sha256 "599b4be55e28d0d7d45acb97779b11e5f7950490361e4e1969f54f18fc3b1f36"
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
