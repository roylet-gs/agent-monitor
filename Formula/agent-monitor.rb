# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.36"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.36/agent-monitor-v2.1.36-darwin-arm64.tar.gz"
      sha256 "d8eff7a7d0b4f35aced0dcacec50c6d2bc758a64706f51b50cf3e81b74efa272"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.36/agent-monitor-v2.1.36-darwin-x64.tar.gz"
      sha256 "8d8ede0d6eddc6b1e77502a8169fcb81c3b9a8dc049cdba838fb15fbc16fc786"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.36/agent-monitor-v2.1.36-linux-arm64.tar.gz"
      sha256 "0598ae43b0ad2de437578f04fa299eff85e635566938ca536f1740a970cb0ff1"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.36/agent-monitor-v2.1.36-linux-x64.tar.gz"
      sha256 "ca0e0a0a2d07f70e250ba266928779a6654c3a0bb8987909fa5f9ee22185915e"
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
