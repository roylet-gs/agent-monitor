# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.35"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.35/agent-monitor-v2.1.35-darwin-arm64.tar.gz"
      sha256 "babd2a50483140a60c912ce728daf2f9a1828cad722e6157b19ce8a503771340"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.35/agent-monitor-v2.1.35-darwin-x64.tar.gz"
      sha256 "44b6a031809099d93f800ed248339acd099e2d983de415c134511b2950b3d04f"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.35/agent-monitor-v2.1.35-linux-arm64.tar.gz"
      sha256 "a3562b51a5698f384111044c13803ad02bacfca95b304ee841274e87c5f61e93"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.35/agent-monitor-v2.1.35-linux-x64.tar.gz"
      sha256 "4a02f6fd95a04db359cf4a4bc6478d426258deecca9cc5d431f0c51c97859a7c"
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
