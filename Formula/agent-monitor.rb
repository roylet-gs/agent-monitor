# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.72"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.72/agent-monitor-v2.1.72-darwin-arm64.tar.gz"
      sha256 "a3768e2911e44cc08c746838439115d3c2d3b80e963eaa04c1f3738766b2ff12"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.72/agent-monitor-v2.1.72-darwin-x64.tar.gz"
      sha256 "ce360b863193931adb49383324f2a729bed6c66ecfba2bff6e2d6b3799cee688"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.72/agent-monitor-v2.1.72-linux-arm64.tar.gz"
      sha256 "0ac3eb58fad2d95369bf8ae140b86bb554d5b1359a4fbadb92dcf38d533f981f"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.72/agent-monitor-v2.1.72-linux-x64.tar.gz"
      sha256 "059118e6cd684850c5d08643c5d0554b8f8effd0e99c7769f634e8eafb05c324"
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
