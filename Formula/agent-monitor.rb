# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.50"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.50/agent-monitor-v2.1.50-darwin-arm64.tar.gz"
      sha256 "846aed8da2a5336acaf41654d77f3465490f621da1d716b6e94082477986050a"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.50/agent-monitor-v2.1.50-darwin-x64.tar.gz"
      sha256 "26ac7e5d8cc8d355ec533ce7047eb8738435d1e5cc1a27d02f64f81913f8524f"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.50/agent-monitor-v2.1.50-linux-arm64.tar.gz"
      sha256 "0861105b808f26c0da50f8a98ffb2af9cfd612da45b4efadafbd9174281bfe45"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.50/agent-monitor-v2.1.50-linux-x64.tar.gz"
      sha256 "d8f3f0788b10f9e658b04484c0e70e4b4b68e8dab76908fb9b9dfdc021f5ad98"
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
