# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.39"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.39/agent-monitor-v2.1.39-darwin-arm64.tar.gz"
      sha256 "5d4e47432d74bf7b5bdb9ff837158b4f88c8e2c72e8fcbff69907e9d19bddaa8"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.39/agent-monitor-v2.1.39-darwin-x64.tar.gz"
      sha256 "49996aef5b2d499c48108c59a8954f51c2b777978bc7f0e8cb8a4909f651609e"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.39/agent-monitor-v2.1.39-linux-arm64.tar.gz"
      sha256 "bfa4cbf25b5a0c594aac4428bf445395f3d6668acf78136ae764c0014765d5cb"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.39/agent-monitor-v2.1.39-linux-x64.tar.gz"
      sha256 "307a9db8b0df8c3293a6317dfe8ca63e435f9e26bbc0fc49f422232a1d170220"
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
