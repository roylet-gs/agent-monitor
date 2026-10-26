# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.44"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.44/agent-monitor-v2.1.44-darwin-arm64.tar.gz"
      sha256 "c4309cf3486757523e49b604189f1a03211ca6d7ba3c4a0aa935f95b15c2e56d"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.44/agent-monitor-v2.1.44-darwin-x64.tar.gz"
      sha256 "a4ca634ca3603a681662372612c1a6f6e18d41d5e71a3f62e45655b94601ac88"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.44/agent-monitor-v2.1.44-linux-arm64.tar.gz"
      sha256 "2134952664516bcd329e1276f195a11ccb1e9d017f54d79fdf3c2b15a4d55748"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.44/agent-monitor-v2.1.44-linux-x64.tar.gz"
      sha256 "d871e4cb3da0dfcd410e92f09f5618ba843b08075a67037d7173f02cb6ee0f2d"
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
