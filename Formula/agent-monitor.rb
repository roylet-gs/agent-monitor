# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.41"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.41/agent-monitor-v2.1.41-darwin-arm64.tar.gz"
      sha256 "c7fc110c9f2d2e0a74c997cf651a3d9d0e173fd6cf9bd4c430d2a0e8a040cdec"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.41/agent-monitor-v2.1.41-darwin-x64.tar.gz"
      sha256 "eef8b9e4f8bd38caf376dc5a5a54f8923b9112528010214e5c1347099da83b97"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.41/agent-monitor-v2.1.41-linux-arm64.tar.gz"
      sha256 "2827b4e62d58645ea0c2facb404a50e28c6e4da4bab659a9e8572be2201649d8"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.41/agent-monitor-v2.1.41-linux-x64.tar.gz"
      sha256 "845d5c420bb4a152816c1b5acb45d14733680b39f6df6a2a002a212ecf4b8f99"
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
