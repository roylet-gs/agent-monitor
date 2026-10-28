# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.60"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.60/agent-monitor-v2.1.60-darwin-arm64.tar.gz"
      sha256 "0e24fe9e293f1e0e783c2a73fe98366757e177eb3fd8fc3bd8ebbc5d2cbfcc8b"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.60/agent-monitor-v2.1.60-darwin-x64.tar.gz"
      sha256 "0cd21f1edf92d0b034dfc09aeda6642596fac2470c249e547a77a46136bb2463"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.60/agent-monitor-v2.1.60-linux-arm64.tar.gz"
      sha256 "cde64205e1e7df557d89fb4be248445a9d43d02f2dedc8e7b97abfee8f723e10"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.60/agent-monitor-v2.1.60-linux-x64.tar.gz"
      sha256 "fc4eec208c532334b39a540bbe9fa1543b9697f45bcfd692228a97d3f2effbb5"
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
