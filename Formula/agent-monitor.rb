# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.58"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.58/agent-monitor-v2.1.58-darwin-arm64.tar.gz"
      sha256 "a0584d3055740aa35135035e335d81cb5b841a7c159fa2fa1537588ce3bcdc6a"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.58/agent-monitor-v2.1.58-darwin-x64.tar.gz"
      sha256 "5e9a1e4a71633eb51034be79dc9dbe952dcd348ca6b616673c3740e7d9a4407e"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.58/agent-monitor-v2.1.58-linux-arm64.tar.gz"
      sha256 "70b4f24f51268206364cfe59fa1a0da0d2368617fea8f3c7f4dcbd5cbbf80768"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.58/agent-monitor-v2.1.58-linux-x64.tar.gz"
      sha256 "a2a1e9d2760dbe17ceafd6f65915afc5fe7494b78db3d6aff8a63d1b621acaa7"
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
