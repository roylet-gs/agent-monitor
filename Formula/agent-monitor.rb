# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.68"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.68/agent-monitor-v2.1.68-darwin-arm64.tar.gz"
      sha256 "f7a83942b979173a318282d0d49567292c4e63ade7bda8b98df8ac7bd5fc301c"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.68/agent-monitor-v2.1.68-darwin-x64.tar.gz"
      sha256 "49c7cdb6328f45e0e70e3faffebf66e1f890edc1599e1203ade6f31f006c4fd5"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.68/agent-monitor-v2.1.68-linux-arm64.tar.gz"
      sha256 "8a7fe2f63db21a3e0ed9c9834b9c6cd4477c046f9f787998a36bb1f4f1ed9529"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.68/agent-monitor-v2.1.68-linux-x64.tar.gz"
      sha256 "0297fa4d6a6bbfde5d914237221ed9dadf428687907eba0da72fec165f87a5c1"
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
