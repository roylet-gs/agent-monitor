# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.66"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.66/agent-monitor-v2.1.66-darwin-arm64.tar.gz"
      sha256 "ed4f84a416568b7437aaa5d131addfddc05912da1ef5f7f2fdc1ca099cdd2d08"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.66/agent-monitor-v2.1.66-darwin-x64.tar.gz"
      sha256 "217840ddeabf40d8baa5f487100ddbb106a69d38f86e77e856eefe29c77f0c9f"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.66/agent-monitor-v2.1.66-linux-arm64.tar.gz"
      sha256 "70222a64206a973b3a59f0b9c4aa1f575fdddb16589f6d7c3290131bfd4b659b"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.66/agent-monitor-v2.1.66-linux-x64.tar.gz"
      sha256 "c89ae371110fb7731abd5d1dfdadf45d6b2c929f45838435219245eb480e7761"
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
