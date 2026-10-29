# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.67"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.67/agent-monitor-v2.1.67-darwin-arm64.tar.gz"
      sha256 "0e7b54a24b8a842ce04d0fe41ef8d5aeee14c76eb10fef102edeab99e5bd8f9e"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.67/agent-monitor-v2.1.67-darwin-x64.tar.gz"
      sha256 "5336b3646603fd70febf7d6b2d7d0754c08a0adb9edf80f851f221dff2c2a650"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.67/agent-monitor-v2.1.67-linux-arm64.tar.gz"
      sha256 "397c440b74bb3772fc04cb8e06febe2aa532131bf7408e49efa7d2e8d34e2f30"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.67/agent-monitor-v2.1.67-linux-x64.tar.gz"
      sha256 "e4eb2be20fc0a67db9c2dab0d399ce5b0864f31392dfc6e6dc14fae83d309ce2"
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
