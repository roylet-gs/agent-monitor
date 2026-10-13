# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.11"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.11/agent-monitor-v2.1.11-darwin-arm64.tar.gz"
      sha256 "7be2dfc32affc545451ebda6fc56ed77ddca5d1890e7a83827ba4d656907b439"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.11/agent-monitor-v2.1.11-darwin-x64.tar.gz"
      sha256 "26f0bbcea9cba28915d46d8f8b179c5feb5a4bb04296a042749e830ccdbff461"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.11/agent-monitor-v2.1.11-linux-arm64.tar.gz"
      sha256 "0ce7b2108886da9d24cb5d4cc2af74033c90376e6cba4e75fe8e198b9e153a38"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.11/agent-monitor-v2.1.11-linux-x64.tar.gz"
      sha256 "8b69acceb7bd0106bb6c0521bcd300729d1052508e108ddedfe1da631df8cd64"
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
