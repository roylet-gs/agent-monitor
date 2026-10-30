# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.73"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.73/agent-monitor-v2.1.73-darwin-arm64.tar.gz"
      sha256 "b44c38acd2e9f33341c5419f8227a602480927993235587043b80e121ad1d8c7"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.73/agent-monitor-v2.1.73-darwin-x64.tar.gz"
      sha256 "078733c2440f25ce688ebe94b23471d2da930ad67768231fe014f8222604fc5f"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.73/agent-monitor-v2.1.73-linux-arm64.tar.gz"
      sha256 "1f47b8e646226540d3774d13858696dd29c38b912bf5091492c7ba93ff4195f4"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.73/agent-monitor-v2.1.73-linux-x64.tar.gz"
      sha256 "51a28e79ef66670e573dc9d7eee82d7c737d7b7eabc718709245f791e381154d"
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
