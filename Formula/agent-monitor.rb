# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.26"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.26/agent-monitor-v2.1.26-darwin-arm64.tar.gz"
      sha256 "8ac964067184fb2a2f1c54397e41011f428e59481f9dad912a02a6bb1293dbf2"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.26/agent-monitor-v2.1.26-darwin-x64.tar.gz"
      sha256 "4aaa6da147832724ec951f3119a256c67b7704c2fe1016d66e940caa8dd2adee"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.26/agent-monitor-v2.1.26-linux-arm64.tar.gz"
      sha256 "19625dee1b06e0e8d27cef7b852fa67d1ef6e27d2de0921d9c5e7f0f0de8e11b"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.26/agent-monitor-v2.1.26-linux-x64.tar.gz"
      sha256 "45ed9fdc72c2da76ba9d63beeaab7e9319630b999b69860926ad5fd06f1ddfd0"
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
