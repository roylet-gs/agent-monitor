# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.23"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.23/agent-monitor-v2.1.23-darwin-arm64.tar.gz"
      sha256 "614459662a547b332f3ef6715390d3e28c24c947ab847071ee0965d9c622dac3"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.23/agent-monitor-v2.1.23-darwin-x64.tar.gz"
      sha256 "53e61ac5e5e0624e37f20aeb73b64e2aba7fbc1759f3356fcbe51768a2b3c284"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.23/agent-monitor-v2.1.23-linux-arm64.tar.gz"
      sha256 "594095602c84ebf6b6e259e288bc522e7552f24f47708ce3200bafd9eb15f008"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.23/agent-monitor-v2.1.23-linux-x64.tar.gz"
      sha256 "41e574d9d3022df1d7ae5659b6dcab0a4f37d2f6cdd7557cde91bb0a985a075c"
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
