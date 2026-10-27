# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.48"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.48/agent-monitor-v2.1.48-darwin-arm64.tar.gz"
      sha256 "d34867ed03584760e8e000a0b0d9d44f569ddb95475ce39d9a7070ab7bce4f4e"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.48/agent-monitor-v2.1.48-darwin-x64.tar.gz"
      sha256 "b13f8f5ced399dfbe6845319f15b7828859bc65ec8bc39f0e46abf2dc6093e57"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.48/agent-monitor-v2.1.48-linux-arm64.tar.gz"
      sha256 "02736e791fc044168f6af80ad7551cbec474f408feb5f96f5af7b9f32794105e"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.48/agent-monitor-v2.1.48-linux-x64.tar.gz"
      sha256 "71789201196a24163f9a6c86a9d3ed3b8e035f4040dfd5a75bb2b0639657428c"
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
