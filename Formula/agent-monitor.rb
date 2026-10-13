# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.13"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.13/agent-monitor-v2.1.13-darwin-arm64.tar.gz"
      sha256 "07ea415f20ea0ad0d71ac87206af9ab26317773bedbd626845f9dbded1132f02"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.13/agent-monitor-v2.1.13-darwin-x64.tar.gz"
      sha256 "814ba493c41270ec1ba7a056c7d47ce32e87271be6b20a0086b72945f87ed400"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.13/agent-monitor-v2.1.13-linux-arm64.tar.gz"
      sha256 "4271146a36829510c6da0509f09c30dd0ce690755aac47ac54ec7241c8e32a6d"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.13/agent-monitor-v2.1.13-linux-x64.tar.gz"
      sha256 "45ec2d35179bac47918c51396e5de8f17a4c000d74bbc15f92df57f7352b41a3"
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
