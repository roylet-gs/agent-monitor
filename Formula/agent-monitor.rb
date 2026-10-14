# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.31"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.31/agent-monitor-v2.1.31-darwin-arm64.tar.gz"
      sha256 "04e3f927ca062388bf05c3364e30d3c14534fe524fa1832650299c4cc7918d73"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.31/agent-monitor-v2.1.31-darwin-x64.tar.gz"
      sha256 "f3ed939b24b6ea9cc348773d40dacb9ed5b272e38342d9e0b9a75c13ed6b348d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.31/agent-monitor-v2.1.31-linux-arm64.tar.gz"
      sha256 "c1eb636f02e409864a9b53475ed53e93e5a5c012abac0b7494314c1e5c933892"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.31/agent-monitor-v2.1.31-linux-x64.tar.gz"
      sha256 "968119bc0188f4aecb8093c85dbf66d04092616983f21d3499b72569b43a8a66"
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
