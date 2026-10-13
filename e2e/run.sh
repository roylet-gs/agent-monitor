#!/bin/bash
# Run the Docker E2E harness under a compose project named after this
# checkout, so several worktrees can run it at once: containers, networks,
# volumes and built images are all prefixed with the project name, and the
# host ports are picked by Docker (the tests reach the app over the compose
# network, not the host).
#
#   e2e/run.sh test    build, run the specs the branch affects (see
#                      e2e/affected-specs.sh), exit with the tests' exit code;
#                      exits 0 without building when no spec is affected
#   e2e/run.sh test --all       build and run every spec
#   e2e/run.sh test 04 09-new   run only these specs (Playwright file filters)
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
    shift || true
    [ "${1:-}" = "--" ] && shift # `pnpm test:e2e -- 04` passes the `--` on
    if [ "${1:-}" = "--all" ]; then
      specs=""
    elif [ $# -gt 0 ]; then
      specs="$*"
    else
      echo "Changed files → E2E specs:"
      specs=$(bash "$root/e2e/affected-specs.sh" --explain)
      if [ -z "$specs" ]; then
        echo "No E2E-relevant changes; skipping the build and the specs (run with --all to force)."
        exit 0
      fi
      [ "$specs" = ALL ] && specs=""
      specs=$(echo $specs) # one line
    fi
    echo "E2E compose project: $project"
    echo "Specs: ${specs:-all}"
    export E2E_SPECS="$specs"
    start=$SECONDS
    "${compose[@]}" build
    built=$SECONDS
    status=0
    "${compose[@]}" up --force-recreate --abort-on-container-exit --exit-code-from tests || status=$?
    echo "E2E timings: build $((built - start))s, specs $((SECONDS - built))s"
    exit $status
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
  *) echo "usage: $0 [test [--all | spec...]|build|clean|ports|logs]" >&2; exit 2 ;;
esac
