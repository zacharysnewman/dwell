#!/bin/sh
# Lists, and with `stop`, ends the game instances an agent started on this Mac (owner, 2026-11-24):
#   instances.sh                 list them
#   instances.sh stop [KIND…]    stop them (all kinds but `app`, or the kinds named)
#   instances.sh stop PID…       stop the listed ones by pid (e.g. one server, keeping another)
# Kinds: vite (dev and preview servers of this repo's client), server (native dwell-server),
# browser (automation Chrome/Chromium: a remote-debugging Chrome, Playwright's, the DevTools MCP's),
# runner (Playwright test runners), master (wrangler / the master server), app (the Dwell Electron
# app: only when named, it may be the owner's). The owner's own Chrome (no remote-debugging flag)
# is never listed or stopped.

classify() {
  # $1: the command line. Prints the kind, or nothing.
  case "$1" in
    *"Google Chrome Helper"*|*"Chromium Helper"*|*"chrome_crashpad"*) ;;
    *"Contents/MacOS/Google Chrome "*|*"Contents/MacOS/Chromium "*|*"headless_shell"*|*"chrome-headless-shell"*)
      case "$1" in
        *--remote-debugging-*|*ms-playwright*|*chrome-devtools-mcp*) echo browser ;;
      esac ;;
    *"node_modules/.bin/vite"*|*"vite preview"*|*"npm exec vite"*) echo vite ;;
    *"/dwell-server"*|*"dwell-server "*) echo server ;;
    *"playwright test"*|*"playwright/cli"*) echo runner ;;
    *wrangler*|*"master/src"*) echo master ;;
    *"Dwell.app/Contents/MacOS"*) echo app ;;
  esac
}

port_of() {
  echo "$1" | sed -n 's/.*--port[ =]\([0-9][0-9]*\).*/:\1/p'
}

list() {
  ps -axo pid=,etime=,command= | while read -r pid etime cmd; do
    # Continuation lines of a multi-line command, and shells (whose text may name anything).
    case "$pid" in ''|*[!0-9]*) continue ;; esac
    [ "$pid" = "$$" ] && continue
    case "$cmd" in /bin/zsh*|/bin/bash*|/bin/sh*|zsh\ *|bash\ *|sh\ *|-zsh*) continue ;; esac
    kind=$(classify "$cmd")
    [ -n "$kind" ] || continue
    printf '%-8s %7s  %-11s %s %s\n' "$kind" "$pid" "$etime" "$(port_of "$cmd")" "$(echo "$cmd" | cut -c1-110)"
  done
}

if [ "${1:-list}" = "list" ]; then
  out=$(list)
  if [ -z "$out" ]; then echo "no game instances running"; else echo "$out"; fi
  exit 0
fi

if [ "$1" != "stop" ]; then
  echo "usage: instances.sh [list | stop [vite|server|browser|runner|master|app|PID]…]" >&2
  exit 2
fi
shift
kinds="${*:-vite server browser runner master}"
pids=$(list | while read -r kind pid rest; do
  for k in $kinds; do [ "$k" = "$kind" ] || [ "$k" = "$pid" ] && echo "$pid"; done
done)
if [ -z "$pids" ]; then echo "nothing to stop ($kinds)"; exit 0; fi
list | while read -r kind pid rest; do
  for k in $kinds; do { [ "$k" = "$kind" ] || [ "$k" = "$pid" ]; } && echo "stopping $kind $pid $rest" | cut -c1-140; done
done
# shellcheck disable=SC2086
kill $pids 2>/dev/null
sleep 2
left=""
for p in $pids; do kill -0 "$p" 2>/dev/null && left="$left $p"; done
# shellcheck disable=SC2086
[ -n "$left" ] && kill -9 $left 2>/dev/null && echo "forced:$left"
echo "left running:"
list | sed 's/^/  /'
