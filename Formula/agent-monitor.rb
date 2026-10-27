# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.46"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.46/agent-monitor-v2.1.46-darwin-arm64.tar.gz"
      sha256 "c20911a08a3484dff99b070e945e6ead5027dc3805c98c2a5165f1eec5f8c839"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.46/agent-monitor-v2.1.46-darwin-x64.tar.gz"
      sha256 "24b335e81cc4dd2eeac81ce79f94f1b9a84a0757cfd59b49abd2a90dc452f993"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.46/agent-monitor-v2.1.46-linux-arm64.tar.gz"
      sha256 "14f8e39fe79714ea9371f34b034fa699bc2f65f5270f6982aae75ff7f835445c"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.46/agent-monitor-v2.1.46-linux-x64.tar.gz"
      sha256 "9581eefb1756877c2219bd722c4cb2a66e504daa541b2254dbcb652822e52ed2"
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
