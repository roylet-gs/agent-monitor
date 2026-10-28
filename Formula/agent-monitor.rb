# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.56"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.56/agent-monitor-v2.1.56-darwin-arm64.tar.gz"
      sha256 "6bac86d4be820e5e60a108f2d12b08fd640f6f72a3b72a787a501a49008c20a6"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.56/agent-monitor-v2.1.56-darwin-x64.tar.gz"
      sha256 "4a0feb819f4f946b4b1968988a8ee8d4df9312e2314d97a5c0774a5e80e76f3d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.56/agent-monitor-v2.1.56-linux-arm64.tar.gz"
      sha256 "e7dcd4d56e9e2dad0543add42a026ae1f5b30661e3a1615f5e0c81f34727a8a4"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.56/agent-monitor-v2.1.56-linux-x64.tar.gz"
      sha256 "38c3e5df4ee55e2727458a8b51dd1effd55223e2d6e9dde2f51e2787accd70dc"
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
