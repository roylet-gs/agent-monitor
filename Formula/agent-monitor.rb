# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.9"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.9/agent-monitor-v2.1.9-darwin-arm64.tar.gz"
      sha256 "44b0a0549ea9640c52f17e28c962d588286cb7ad749f4e0021c185345f892563"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.9/agent-monitor-v2.1.9-darwin-x64.tar.gz"
      sha256 "040d7e9fb0582c79e62c7071be351f01ed68acc9ef90cbecd894f55d476edbf3"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.9/agent-monitor-v2.1.9-linux-arm64.tar.gz"
      sha256 "38579ee5b2f93f4158b26a077c9cc12ed33823e56e2c907182534fb09138cc39"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.9/agent-monitor-v2.1.9-linux-x64.tar.gz"
      sha256 "bba12a756dde466581e103b8022a79abc07f99d810e03cee7acfd247a5eecb50"
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
