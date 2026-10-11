# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.0"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.0/agent-monitor-v2.1.0-darwin-arm64.tar.gz"
      sha256 "4271ae9c2f674089beeeb65074e5d3adb7363edb89b8066f0f4d1bd2a682b816"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.0/agent-monitor-v2.1.0-darwin-x64.tar.gz"
      sha256 "08227cbe6232ab49fc62bd23a790bd72c2054174259b232a8ce06e25f0ab0222"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.0/agent-monitor-v2.1.0-linux-arm64.tar.gz"
      sha256 "59e0210d26b3c9052b959e5390f92848cee32db0c03e1cf78f014423908e4d70"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.0/agent-monitor-v2.1.0-linux-x64.tar.gz"
      sha256 "5fecce3aad399c45a92f897b9f6b03ff93bd049e136bfd84ea74e46dd2ec328d"
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
