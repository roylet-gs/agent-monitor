# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.80"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.80/agent-monitor-v2.1.80-darwin-arm64.tar.gz"
      sha256 "48dc5c62a7952dc78257abe0de947425976576258fbfde36e06d1e4baef975b4"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.80/agent-monitor-v2.1.80-darwin-x64.tar.gz"
      sha256 "8d019111798051e4ea2bf34e02ebff648c52578dea8bfb3d72b3fac2549cb93b"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.80/agent-monitor-v2.1.80-linux-arm64.tar.gz"
      sha256 "0d3d2fb743c453e1706c04715563beed1a358482fd2d73a3ebaaccc2e65b7ee6"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.80/agent-monitor-v2.1.80-linux-x64.tar.gz"
      sha256 "96417b5476c86b7aec0bd80632a28c1d79ae1656a97fefa7553039401452aa62"
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
