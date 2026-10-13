# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.10"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.10/agent-monitor-v2.1.10-darwin-arm64.tar.gz"
      sha256 "c383ce015dba5bd3f32abd7d31d4d919e55390bd96cea963df2c5dd91d8dc310"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.10/agent-monitor-v2.1.10-darwin-x64.tar.gz"
      sha256 "d7fc4df3c46c5f3e6cac2bd6310fcb4424486571506ccf53d0aecde1f79b68c3"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.10/agent-monitor-v2.1.10-linux-arm64.tar.gz"
      sha256 "fad4d0068188699d1e6004b30887b51b82befcc029e0dcf9608d6dd87ae14b04"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.10/agent-monitor-v2.1.10-linux-x64.tar.gz"
      sha256 "ede77248fd540c892cd07b0e3d674c6c2fa4ad07e1e9e29bcbd46f600f448895"
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
