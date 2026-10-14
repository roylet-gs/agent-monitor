# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.28"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.28/agent-monitor-v2.1.28-darwin-arm64.tar.gz"
      sha256 "5f10449f7b7be04c8438fe304c86af7632cb96e55ed0afb0c8ae104bdad13f92"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.28/agent-monitor-v2.1.28-darwin-x64.tar.gz"
      sha256 "dadc3091f6059bb4d0a81874a0b29c8e9e437b19f81689ae8115a2814ca10c8d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.28/agent-monitor-v2.1.28-linux-arm64.tar.gz"
      sha256 "d2310d9a6a0aa1d6366710ff053007ac23b9aeb459bcdf72dbd9e51dc6367f7d"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.28/agent-monitor-v2.1.28-linux-x64.tar.gz"
      sha256 "8088ef13c0a9b86263d608542b68c5330ee86d0d03fcc2ff93ce993736ee5faa"
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
