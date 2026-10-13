# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.12"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.12/agent-monitor-v2.1.12-darwin-arm64.tar.gz"
      sha256 "230b1a6f2dd2558fe7647364a80debebc1f97dfa48dfbb659e10ccffad890bf4"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.12/agent-monitor-v2.1.12-darwin-x64.tar.gz"
      sha256 "844abaaa9d3f7b977d0240417e8ac31df1537d7c1efcee0a8f35f9ef47fcbcf8"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.12/agent-monitor-v2.1.12-linux-arm64.tar.gz"
      sha256 "eaaf883fb5c28bafa59f646c563c2a56e64620017fd7a12ad5e1c905521b50ec"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.12/agent-monitor-v2.1.12-linux-x64.tar.gz"
      sha256 "3a7c22af09071fc4cfd9bea6af0fb3306bbe28e180d27ff9fc97da46a501f7c1"
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
