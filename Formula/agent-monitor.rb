# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.77"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.77/agent-monitor-v2.1.77-darwin-arm64.tar.gz"
      sha256 "f9c88cd39c937db5797bd7c50d2c5f45a04f9c567b60af5a4b38fe43a9a7a32e"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.77/agent-monitor-v2.1.77-darwin-x64.tar.gz"
      sha256 "2b8d73cf5c0182dc9264b9b23c546484a931fc2db3cf5addc9b832dc07640878"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.77/agent-monitor-v2.1.77-linux-arm64.tar.gz"
      sha256 "7288925b34ec6261d16ea8521db307fad3516412ede3db716fb292cf30b8cc07"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.77/agent-monitor-v2.1.77-linux-x64.tar.gz"
      sha256 "4a2e60bb875a8a4ca88c5f5954813b36178a1f7055937d04c1b971cc46737213"
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
