# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.57"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.57/agent-monitor-v2.1.57-darwin-arm64.tar.gz"
      sha256 "17a206a7a3cb787a9ec2415d051c87134c35ad704152472afd8a7638e80f5d96"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.57/agent-monitor-v2.1.57-darwin-x64.tar.gz"
      sha256 "ca1baa9f5714bd734da03ec4296bc27c81579ef9df3ff2e335dbb991f3f2e67b"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.57/agent-monitor-v2.1.57-linux-arm64.tar.gz"
      sha256 "915d5dca60f6608ffb6655fd1eaba425c27589579aa7ee691890e530f2614920"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.57/agent-monitor-v2.1.57-linux-x64.tar.gz"
      sha256 "88dedb767076d6cb26db03a5634caab14bab119f2802e7637f60fd8076bf9b32"
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
