# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.38"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.38/agent-monitor-v2.1.38-darwin-arm64.tar.gz"
      sha256 "8ae23a27816569ff0222768ef3c500e3e7b47b87d8c00e618ae8162f717e27d5"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.38/agent-monitor-v2.1.38-darwin-x64.tar.gz"
      sha256 "5e63d107041db82b8d63685c807718a3908b04ad93fa2d2df6cc1ae002fe9926"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.38/agent-monitor-v2.1.38-linux-arm64.tar.gz"
      sha256 "775dc53d3632db3c78986b8f1f315564874d78796290f9fdc38568b21ff0ed2a"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.38/agent-monitor-v2.1.38-linux-x64.tar.gz"
      sha256 "ddac804b83c220ae29a887c472ea9747387179cabdc1882f89f4e422c3134d0a"
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
