# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.32"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.32/agent-monitor-v2.1.32-darwin-arm64.tar.gz"
      sha256 "c40e9e1737efb73303058d62a6e3b4af73d4f739b15cda6f3a5027d8951a3457"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.32/agent-monitor-v2.1.32-darwin-x64.tar.gz"
      sha256 "93ea0f8d9427028f31e6c68f3dde0f24ec46d4aa9464c121fe82a09bdcfc8992"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.32/agent-monitor-v2.1.32-linux-arm64.tar.gz"
      sha256 "e4e31714cfa6334d5cc0b903799b46fba0ba3b0f7d4af1142216f7cde070008c"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.32/agent-monitor-v2.1.32-linux-x64.tar.gz"
      sha256 "6e278b424ee2a381e622cdd13c6bfc9dd6a4607c772a8b61ef8709d7e82eb481"
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
