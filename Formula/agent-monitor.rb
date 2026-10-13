# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.8"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.8/agent-monitor-v2.1.8-darwin-arm64.tar.gz"
      sha256 "935db1048fa304f1446139957f3bae2f22f6062103e8b257320c0576c91dac63"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.8/agent-monitor-v2.1.8-darwin-x64.tar.gz"
      sha256 "11d53672978d25a1c47210e80aab775c63eb7fc435219cf119fa4856cad206b5"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.8/agent-monitor-v2.1.8-linux-arm64.tar.gz"
      sha256 "2c1fcd43a26d7243324f72808a39f10a7ac6dfcd1c019e700064e054ba70daf1"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.8/agent-monitor-v2.1.8-linux-x64.tar.gz"
      sha256 "8021173093f0b9ca436f26398edcdcfef319f8ad15c7bd1456a423a5d733ac93"
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
