# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.70"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.70/agent-monitor-v2.1.70-darwin-arm64.tar.gz"
      sha256 "78a41635a72ac8ffdd7290ee3e73ed1668670b119c163b5139ba6385a4273842"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.70/agent-monitor-v2.1.70-darwin-x64.tar.gz"
      sha256 "b6e19611ce3fe18654b8444fee2277e1d622aa69f26df134b02c205d6fe67134"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.70/agent-monitor-v2.1.70-linux-arm64.tar.gz"
      sha256 "5762dbeba14419a33baaf52b7b33e4831fb5b04fb3dcc052d10ffc2218f9fd13"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.70/agent-monitor-v2.1.70-linux-x64.tar.gz"
      sha256 "57de0fbd8e604ac9d98016eb2ca901d9d0615969183d8b49becd340670197393"
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
