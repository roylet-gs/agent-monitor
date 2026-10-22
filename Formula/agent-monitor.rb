# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.40"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.40/agent-monitor-v2.1.40-darwin-arm64.tar.gz"
      sha256 "c018540aa3474e4d19a5731dba5837070059e915600dc27292778954877c3751"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.40/agent-monitor-v2.1.40-darwin-x64.tar.gz"
      sha256 "00eb4b1fe66522b10c7d34f826134ea3de1c3fb9ecf3c1c49904ce00795f41a0"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.40/agent-monitor-v2.1.40-linux-arm64.tar.gz"
      sha256 "30fe05e45c27dd1b099c0d2b5dd775c5588879b08e7cf70f388a53de289df595"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.40/agent-monitor-v2.1.40-linux-x64.tar.gz"
      sha256 "185c29bc4fc2f04ca497783b598ec902014fe61e185f1b4cbe0d6fad879b3cd9"
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
