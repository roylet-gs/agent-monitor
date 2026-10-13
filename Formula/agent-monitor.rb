# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.4"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.4/agent-monitor-v2.1.4-darwin-arm64.tar.gz"
      sha256 "a28df662cfca32cc483712f5cf9a5acfa3234027600c7cabdba9f1c6ab26fc4c"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.4/agent-monitor-v2.1.4-darwin-x64.tar.gz"
      sha256 "d9d4891807e689562efa0f6341056f1f579a9a7d137fbc51954544456a1cd6f9"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.4/agent-monitor-v2.1.4-linux-arm64.tar.gz"
      sha256 "9f1a064439b702cb054e625d7ead7dca614f7a612ac75acd31b24d8df22561a7"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.4/agent-monitor-v2.1.4-linux-x64.tar.gz"
      sha256 "81767cd1537b3559b053eb7c26904005e8be010a90f723842caf35c5cee15a24"
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
