# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.42"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.42/agent-monitor-v2.1.42-darwin-arm64.tar.gz"
      sha256 "18564360dd916e615d04af4792d16ee31a71627c42da767b0721afd63e7a1cf1"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.42/agent-monitor-v2.1.42-darwin-x64.tar.gz"
      sha256 "deec2d29cab951cf8d487e00aeed3f8616a75bebae3099093dcf526fb48c720b"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.42/agent-monitor-v2.1.42-linux-arm64.tar.gz"
      sha256 "69d7fcc351ff2b3e3c8c1fa54dd596dd962552fdc5b0570eabe1aad8d5332a9e"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.42/agent-monitor-v2.1.42-linux-x64.tar.gz"
      sha256 "080e541e6ce81182842b2d97857722d2fdfb0ef9fa1cdf1ac23e21a54c8e2c78"
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
