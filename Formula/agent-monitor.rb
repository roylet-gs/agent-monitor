# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.47"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.47/agent-monitor-v2.1.47-darwin-arm64.tar.gz"
      sha256 "a76f7d003dcf297b2853e8017fb979fb1d842df71c8cfd8504b7712a610d4894"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.47/agent-monitor-v2.1.47-darwin-x64.tar.gz"
      sha256 "9dff9ac0e7c69316e14737412c7e6a6c7f09ed846697c7b3d516d5c9f96287a6"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.47/agent-monitor-v2.1.47-linux-arm64.tar.gz"
      sha256 "1ad3b24f17b5b88735fb8c5acc134de3e2f55df41ab451342f7ba399552b5cd7"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.47/agent-monitor-v2.1.47-linux-x64.tar.gz"
      sha256 "8f26ffbcdc1a88e062ed67943d93411c98e105afb7e2e99ac682ee12c51bef3e"
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
