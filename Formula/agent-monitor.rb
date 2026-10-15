# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.34"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.34/agent-monitor-v2.1.34-darwin-arm64.tar.gz"
      sha256 "394e90ff9869a5949daee1c67a2c9f9014cd0035e104e7bfd7bbd7e438b2cfeb"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.34/agent-monitor-v2.1.34-darwin-x64.tar.gz"
      sha256 "0e6381a51d0763d467ab9acec242e771124c7e085deb41d1f33b2f0d7fac999b"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.34/agent-monitor-v2.1.34-linux-arm64.tar.gz"
      sha256 "faea50ade3baf12b250a21eceac6bc466e521405f8b0afa5dac7e331d9580dd2"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.34/agent-monitor-v2.1.34-linux-x64.tar.gz"
      sha256 "4f22712a85f79041d048e5e8503dd6b98026d7c9614c4745612f914121dd14c6"
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
