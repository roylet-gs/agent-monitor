# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.22"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.22/agent-monitor-v2.1.22-darwin-arm64.tar.gz"
      sha256 "13e113d9cd38b30d0acedd74454b469826ad54d16a24157e0cea352ef0b53392"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.22/agent-monitor-v2.1.22-darwin-x64.tar.gz"
      sha256 "46746198143027c100e3c2425b5c6c332ff7213dce7a174b6e3abf29865a298c"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.22/agent-monitor-v2.1.22-linux-arm64.tar.gz"
      sha256 "a9532ca480087e797a2242873c01b97fcd18d01d81d240afc0c89dd61cd0f875"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.22/agent-monitor-v2.1.22-linux-x64.tar.gz"
      sha256 "4e84f0229efdb50073b2a63be86958020a8b6528ecd2800056127876ed7b7baa"
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
