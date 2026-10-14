# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.33"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.33/agent-monitor-v2.1.33-darwin-arm64.tar.gz"
      sha256 "30a06f1e4e1c3a6cb530c2548e46b3fc89fdec7eadcf1070a39f38c3185138d7"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.33/agent-monitor-v2.1.33-darwin-x64.tar.gz"
      sha256 "d51e86b8850b484fbcbfe028fa4b21e4236cbf93a589fb0d53f2980907c7d0cd"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.33/agent-monitor-v2.1.33-linux-arm64.tar.gz"
      sha256 "5a05dc542aafca736e08955811d5d850e9d16e306409c05264340a812412f18c"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.33/agent-monitor-v2.1.33-linux-x64.tar.gz"
      sha256 "49b5e0fe170672a68d13773b89d8bc75aa595a1fa61dd5b20120f6603b80fcf2"
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
