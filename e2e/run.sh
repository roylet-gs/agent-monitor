#!/bin/bash
# Run the Docker E2E harness under a compose project named after this
# checkout, so several worktrees can run it at once: containers, networks,
# volumes and built images are all prefixed with the project name, and the
# host ports are picked by Docker (the tests reach the app over the compose
# network, not the host).
#
#   e2e/run.sh test    build, run the specs, exit with the tests' exit code
#   e2e/run.sh build   build the images only
#   e2e/run.sh clean   tear down containers, local images and volumes
#   e2e/run.sh ports   print the host ports of a running stack (debugging)
#   e2e/run.sh logs    container logs (after a failed run, before clean)
#
# Override the project name with AM_E2E_PROJECT.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
name=${AM_E2E_PROJECT:-am-e2e-$(basename "$root")}
# Compose project names: lowercase letters, digits, dashes and underscores.
project=$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9_-' '-')
compose=(docker compose -p "$project" -f "$root/e2e/docker-compose.yml")

case "${1:-test}" in
  test)
    echo "E2E compose project: $project"
    "${compose[@]}" up --build --force-recreate --abort-on-container-exit --exit-code-from tests
    ;;
  build) "${compose[@]}" build ;;
  clean) "${compose[@]}" down -v --rmi local ;;
  logs) "${compose[@]}" logs --no-color ;;
  ports)
    for p in "app 7681" "app 7682" "mock-api 4100"; do
      # shellcheck disable=SC2086
      echo "$p → $("${compose[@]}" port $p)"
    done
    ;;
  *) echo "usage: $0 [test|build|clean|ports|logs]" >&2; exit 2 ;;
esac
