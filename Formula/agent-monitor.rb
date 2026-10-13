# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.19"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.19/agent-monitor-v2.1.19-darwin-arm64.tar.gz"
      sha256 "44cc5d55532535b57cf3b8db23b8af52074d3cd5b2db78d704282ebd25cd27ae"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.19/agent-monitor-v2.1.19-darwin-x64.tar.gz"
      sha256 "181368c64eba294b764dca6f089e6f7eeb2184e6e5e816484c368b7735c810ff"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.19/agent-monitor-v2.1.19-linux-arm64.tar.gz"
      sha256 "3d8a2381b8b059bf2221066aa21c6c69ba33986261ffcfbb80fa4f24c070f36b"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.19/agent-monitor-v2.1.19-linux-x64.tar.gz"
      sha256 "dd18d6792bcb2fda3b233ad8554a1744a8b4bb0d9c3564cc05ed33ff054c2498"
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
