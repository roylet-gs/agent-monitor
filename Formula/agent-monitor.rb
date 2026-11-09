# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.82"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.82/agent-monitor-v2.1.82-darwin-arm64.tar.gz"
      sha256 "5776df4cc62993fdc0f3bbb8c5bcbbefbdb0ce1aa6224cc7e64368bd831d6e0b"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.82/agent-monitor-v2.1.82-darwin-x64.tar.gz"
      sha256 "a5556f4d75b0ee5b0c87ceb9df7e2a33ef634a48c2a8eb1c128b48b721748379"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.82/agent-monitor-v2.1.82-linux-arm64.tar.gz"
      sha256 "46f62a989c0a17001f86f674a5c56abc87e35e617bad0db1a5f2c88ee3fe3a96"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.82/agent-monitor-v2.1.82-linux-x64.tar.gz"
      sha256 "499b3c246765b1138f4228c69f22c6046080f4cece84cc7c157ffa1b1bd4e62a"
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
