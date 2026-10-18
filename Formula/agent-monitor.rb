# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.37"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.37/agent-monitor-v2.1.37-darwin-arm64.tar.gz"
      sha256 "181fcb2f75aa16dcc0f2f0824e9a05c79dbca4099a888b98e9b0ab7d5e460658"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.37/agent-monitor-v2.1.37-darwin-x64.tar.gz"
      sha256 "3fd17f8a179e4101c7e1c1e704b19c6d59837a32c806ec08484d19f01d28d84d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.37/agent-monitor-v2.1.37-linux-arm64.tar.gz"
      sha256 "b4191c30d909126e09311c56738b9cd0657625a43f20b7fd4a2a111897ff1b41"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.37/agent-monitor-v2.1.37-linux-x64.tar.gz"
      sha256 "399f890a180404406dfbe51afc1232042766447aa5e7a23df2cab3a9aa8384d0"
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
