# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.64"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.64/agent-monitor-v2.1.64-darwin-arm64.tar.gz"
      sha256 "9a2b99a0c169fd3626327b3271ad7ecb580f72d17e5aa46c32086cbf2dc64c77"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.64/agent-monitor-v2.1.64-darwin-x64.tar.gz"
      sha256 "b584a7808c77473bbdaa20e0582d9445a111416eed98cac9cfb5c25cb5515c8d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.64/agent-monitor-v2.1.64-linux-arm64.tar.gz"
      sha256 "a02e20cddc4bf387e48175aa72ab5a8d145402a506d2dce2e8eb97af4fdf9c4c"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.64/agent-monitor-v2.1.64-linux-x64.tar.gz"
      sha256 "6a0156480b1facd2286e4f7e61b296acd66c32ee1fb4a6272a670fcccb8e66fc"
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
