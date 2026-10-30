# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.74"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.74/agent-monitor-v2.1.74-darwin-arm64.tar.gz"
      sha256 "55275d55d7516912bff9e4c243d5460394f987c9f52b95562d0109b682cad5b3"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.74/agent-monitor-v2.1.74-darwin-x64.tar.gz"
      sha256 "cee38e65cf1cf552f5e1477b72721b174bd9f48b791965d7e7f75f7ec16b5ba5"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.74/agent-monitor-v2.1.74-linux-arm64.tar.gz"
      sha256 "ac40391ee022a726c7a5136851c4fb9c7e9524569dc3f31d4b052be395ca0b69"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.74/agent-monitor-v2.1.74-linux-x64.tar.gz"
      sha256 "0f6507d0b33bc2930f392a68c52d663748ec876d5b956e9ed4add1558edc060c"
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
