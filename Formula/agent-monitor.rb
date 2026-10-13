# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.6"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.6/agent-monitor-v2.1.6-darwin-arm64.tar.gz"
      sha256 "f9aa6d4a6fd1909e4c7774d3d92fab2131e1181f269051915d49bc60fc21a249"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.6/agent-monitor-v2.1.6-darwin-x64.tar.gz"
      sha256 "69ff35a598b3a82ce6b612fcd23b0529850cbb024171e48abd350601673da0aa"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.6/agent-monitor-v2.1.6-linux-arm64.tar.gz"
      sha256 "acc01d9b3efc318644e09a67856bf415550176631e5ed42f8c96002a3c007e19"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.6/agent-monitor-v2.1.6-linux-x64.tar.gz"
      sha256 "d633d4418df134c82cdb80898ac0b3ff416ce0c88e94b1ff1c8a0579f0240159"
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
