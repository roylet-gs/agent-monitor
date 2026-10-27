# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.52"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.52/agent-monitor-v2.1.52-darwin-arm64.tar.gz"
      sha256 "149e0b4706dcb8c6e5d242e1e6fb39e7397acda70fc9e72d468b14d1b54d41f5"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.52/agent-monitor-v2.1.52-darwin-x64.tar.gz"
      sha256 "c4fc83b3c8210997c9ff527174ca7a1cd4e182ed1680907faaf6ba3519a1d06d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.52/agent-monitor-v2.1.52-linux-arm64.tar.gz"
      sha256 "d00cce5088e47be10249dc2944c7705409d688082dab18ef80f57c8b5e1e9f60"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.52/agent-monitor-v2.1.52-linux-x64.tar.gz"
      sha256 "4bbdd1c5152316b2ce762b32d3711f0be7b77a5fbf811108cfefdb43b277f381"
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
