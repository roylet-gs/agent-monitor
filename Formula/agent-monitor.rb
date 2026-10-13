# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.17"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.17/agent-monitor-v2.1.17-darwin-arm64.tar.gz"
      sha256 "e187a060632709849fe849a735a4993971b9a4430cd952a826a4f90ec0201ec2"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.17/agent-monitor-v2.1.17-darwin-x64.tar.gz"
      sha256 "64a96431b568f9709f0894268e8d915110754e53d9797379e783b4b519486dfc"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.17/agent-monitor-v2.1.17-linux-arm64.tar.gz"
      sha256 "a24c7e52a19c3a4d9d022e5fd17b57377d42577fd4570555a3cf50617890b25d"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.17/agent-monitor-v2.1.17-linux-x64.tar.gz"
      sha256 "ca1876d862c2ba4ce7263a3be16cf4c4add27e797d9690b0e1d1a176b0a06f91"
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
