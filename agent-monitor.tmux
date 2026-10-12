#!/usr/bin/env bash
# agent-monitor tmux plugin (TPM): `set -g @plugin 'roylet-gs/agent-monitor'`.
#
# Options (set before TPM runs):
#   @am-popup-key      prefix key for the dashboard popup          (default: A)
#   @am-sidebar-key    prefix key to toggle the sidebar            (default: S)
#   @am-sidebar-width  sidebar width in columns                    (default: 44)
#   @am-sidebar-side   left or right                               (default: left)
#   @am-passthrough    on: let OSC 9 notifications reach the outer
#                      terminal (sets allow-passthrough)           (default: on)
#   @am-bin            directory holding am and am-tui             (default: $PATH)
#
# Status bar: put #{agent_monitor} in status-right or status-left; it becomes
# the agent counts from `am summary --format tmux` (e.g. ◐1 ●2).

get() {
  local v
  v="$(tmux show-option -gqv "$1")"
  echo "${v:-$2}"
}

bin_dir="$(get @am-bin "")"
if [ -n "$bin_dir" ]; then
  am="$bin_dir/am"
  tui="$bin_dir/am-tui"
else
  am="$(command -v am)"
  tui="$(command -v am-tui)"
fi
if [ -z "$am" ] || [ ! -x "$am" ] || [ -z "$tui" ] || [ ! -x "$tui" ]; then
  tmux display-message "agent-monitor: am / am-tui not found (install agent-monitor or set @am-bin)"
  exit 0
fi

q() { printf "'%s'" "${1//\'/\'\\\'\'}"; }

popup_key="$(get @am-popup-key A)"
sidebar_key="$(get @am-sidebar-key S)"
width="$(get @am-sidebar-width 44)"
side="$(get @am-sidebar-side left)"

tmux bind-key "$popup_key" display-popup -E -w 85% -h 85% -d '#{pane_current_path}' "$(q "$tui") --popup"
tmux bind-key "$sidebar_key" run-shell "$(q "$am") sidebar --width $width --position $side --target '#{pane_id}'"

if [ "$(get @am-passthrough on)" = "on" ]; then
  tmux set-option -g allow-passthrough on
fi

segment="#($(q "$am") summary --format tmux)"
for opt in status-right status-left; do
  value="$(tmux show-option -gqv "$opt")"
  if [[ "$value" == *"#{agent_monitor}"* ]]; then
    tmux set-option -g "$opt" "${value//\#\{agent_monitor\}/$segment}"
  fi
done
