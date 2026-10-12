# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.1"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.1/agent-monitor-v2.1.1-darwin-arm64.tar.gz"
      sha256 "65f257b0b5d3ce24463e0f298b40308b7607182612025daa8764cb38d4e7f569"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.1/agent-monitor-v2.1.1-darwin-x64.tar.gz"
      sha256 "a90f373c10dcb2e5d368436b0fbc5d0fd564a81289ddfa44cde48196ffbd582d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.1/agent-monitor-v2.1.1-linux-arm64.tar.gz"
      sha256 "72192f4c1be04d61f7f24fe12ca4fb7482f748e8d7f31f6ad010c5c5d7469f7c"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.1/agent-monitor-v2.1.1-linux-x64.tar.gz"
      sha256 "882142ae8101f7b8a1330012044b739b09e6ce5e1325d8eff812c9542ace4216"
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
