# Template for Formula/agent-monitor.rb, which publish.yml regenerates on every
# release (version and tarball checksums filled in). The formula lives in this repo so
# it can be tapped directly:
#   brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
#   brew install agent-monitor
class AgentMonitor < Formula
  desc "Terminal dashboard for Claude Code agents across git worktrees"
  homepage "https://github.com/roylet-gs/agent-monitor"
  version "2.1.54"

  on_macos do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.54/agent-monitor-v2.1.54-darwin-arm64.tar.gz"
      sha256 "88412e996be01b1ef4dc7a1569d23f936e070fc7b24bacac46dd7a285fb86728"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.54/agent-monitor-v2.1.54-darwin-x64.tar.gz"
      sha256 "f637829a4248d75b9251179eb80645c82847112f339343c747f65817eb0bb52d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.54/agent-monitor-v2.1.54-linux-arm64.tar.gz"
      sha256 "6d486d5f49f159598045791856a8a398a2463a5194eba0f27b3c8c40486e1ecb"
    end
    on_intel do
      url "https://github.com/roylet-gs/agent-monitor/releases/download/v2.1.54/agent-monitor-v2.1.54-linux-x64.tar.gz"
      sha256 "171c2e5417ef15a990e442986b9d8b79e7fb3e3a54b4b835c42093c11482c6d2"
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
