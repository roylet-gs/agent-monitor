# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.86"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.86/agent-monitor-v2.1.86-darwin-arm64.tar.gz"
      sha256 "a1aad71527425317a20ef2d91ff10392a1aa70855f503a0c0f2a67497e6bcc86"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.86/agent-monitor-v2.1.86-darwin-x64.tar.gz"
      sha256 "16a106f3bf8ae82d6ce28d816054bcb920e5108abef0378faa4b384558c0aa33"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.86/agent-monitor-v2.1.86-linux-arm64.tar.gz"
      sha256 "a11b2a62709d2a98510cfc593b87152762f4842ea47cd60b8dc535926b84d110"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.86/agent-monitor-v2.1.86-linux-x64.tar.gz"
      sha256 "d73a75a2bceb672d90d6c04ecc7556fc44b46b0fa9b55b85572ba7a89ab4644a"
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
