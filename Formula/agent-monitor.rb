# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.30"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.30/agent-monitor-v2.1.30-darwin-arm64.tar.gz"
      sha256 "2a1ca6dcb4860f23ab9eaa7c13591a220f5979b5c0ff79ad05f64c5a79915e58"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.30/agent-monitor-v2.1.30-darwin-x64.tar.gz"
      sha256 "0905b1b00771fe289fefed4093a2b32a7a7db9a2761f10e28ac2eb6919976bc9"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.30/agent-monitor-v2.1.30-linux-arm64.tar.gz"
      sha256 "bbcd5e70f8e59de7d5ce0d26c30d8cbe63b97791264567d25808f5d20871e23c"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.30/agent-monitor-v2.1.30-linux-x64.tar.gz"
      sha256 "bb289697e08b00979d8736dfa7b12f64f4960dc0d2206ddf6e4dfec8a85a3103"
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
