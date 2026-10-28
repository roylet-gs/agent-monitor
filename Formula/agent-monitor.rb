# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.55"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.55/agent-monitor-v2.1.55-darwin-arm64.tar.gz"
      sha256 "b02dfb91ab2209f9544c340c24e8de30366abac97ab65a9ab5944b4584a16da6"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.55/agent-monitor-v2.1.55-darwin-x64.tar.gz"
      sha256 "4a95ce11621816bb722533fedc663cc589f05e95fbf960206d19ec4410cd63cb"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.55/agent-monitor-v2.1.55-linux-arm64.tar.gz"
      sha256 "39b3bd34fc08d1ce1a7ad06dc103d7b995d195d08f265fe86bf9d651e34ef22e"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.55/agent-monitor-v2.1.55-linux-x64.tar.gz"
      sha256 "1a0886ed2217bcd3f36ebbe695d332dae3facc460048d53a49806508e972a467"
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
