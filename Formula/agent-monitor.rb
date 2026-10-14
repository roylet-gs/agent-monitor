# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.25"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.25/agent-monitor-v2.1.25-darwin-arm64.tar.gz"
      sha256 "a74e298e360bacbf445b37b7803c606313da4abd88bcb87a1bfb857bbef93001"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.25/agent-monitor-v2.1.25-darwin-x64.tar.gz"
      sha256 "3ab565d808d2ef9e43e6ab4ba532899a09264375ac664888347a8a2ccc81e1d0"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.25/agent-monitor-v2.1.25-linux-arm64.tar.gz"
      sha256 "e6ac8b28521e95ada10abbf6f60ef746bae837d91b6c02aab9d4a29b607a899d"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.25/agent-monitor-v2.1.25-linux-x64.tar.gz"
      sha256 "2fa0caca5b39c0acdb056892cadcfe86e8e8190f9412503eeabbd682bd02f3fa"
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
