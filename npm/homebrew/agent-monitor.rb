# Template — fill VERSION and the sha256s from a `release-native` run, then
# commit to the roylet-gs/homebrew-tap repo as Formula/agent-monitor.rb.
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "VERSION"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/vVERSION/agent-monitor-vVERSION-darwin-arm64.tar.gz"
      sha256 "SHA256_DARWIN_ARM64"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/vVERSION/agent-monitor-vVERSION-darwin-x64.tar.gz"
      sha256 "SHA256_DARWIN_X64"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/vVERSION/agent-monitor-vVERSION-linux-arm64.tar.gz"
      sha256 "SHA256_LINUX_ARM64"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/vVERSION/agent-monitor-vVERSION-linux-x64.tar.gz"
      sha256 "SHA256_LINUX_X64"
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
