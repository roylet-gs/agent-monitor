# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.59"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.59/agent-monitor-v2.1.59-darwin-arm64.tar.gz"
      sha256 "03d7511dede48ad31ae29ecde245066983f6ef7023901fe807b9ec37e7789200"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.59/agent-monitor-v2.1.59-darwin-x64.tar.gz"
      sha256 "d6d511cc827af7e4939c98ccf72ada144c6017803ae527864873c26074e99acf"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.59/agent-monitor-v2.1.59-linux-arm64.tar.gz"
      sha256 "e87386ef5982a93de8bee21d0168db7710f9372d4637d24bb543bf53640a8528"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.59/agent-monitor-v2.1.59-linux-x64.tar.gz"
      sha256 "32bdad78e659f8eee8ee364512c60276b6163ebc9d983a3c2b9652f35fe9a87b"
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
