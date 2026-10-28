# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.62"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.62/agent-monitor-v2.1.62-darwin-arm64.tar.gz"
      sha256 "c10d12026b6cdd3a0306465db633d36ca8959cd1c95f48af5694bc7641b2a509"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.62/agent-monitor-v2.1.62-darwin-x64.tar.gz"
      sha256 "0ea9285b93f7edfbbd3ad8092c652b5530245ba543758b2f44aad9700bc46b7d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.62/agent-monitor-v2.1.62-linux-arm64.tar.gz"
      sha256 "69d35682376d079a9e0c5336e38de02fe309899aff4a82770b28ecd5e7f0fff1"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.62/agent-monitor-v2.1.62-linux-x64.tar.gz"
      sha256 "1c6146057397bd07964a0ea69d728927fae9db48f72786d7dea5a8d031a70a2f"
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
