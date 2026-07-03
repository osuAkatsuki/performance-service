#!/usr/bin/env bash
set -euo pipefail

COMPOSE_DIR="${COMPOSE_DIR:-/opt/akatsuki}"
COMPOSE_SERVICE="${COMPOSE_SERVICE:-performance-service-api}"
LOGS_DIR="${LOGS_DIR:-/opt/akatsuki/logs}"

NAME=""
MODES="0,1,2,3"
RELAX_BITS="0,1,2"
TOTAL_PP_ONLY="0"
TOTAL_PP="1"
MAP_FILTER=""
MODS_FILTER=""
NEQ_MODS_FILTER=""
MAPPER_FILTER=""
PP_ZERO=0
AFTER_DATE=""
AFTER_TIME=""
EXECUTE=0
PULL_IMAGE=0

usage() {
  cat <<EOF
Usage: $0 [options]

Runs the performance-service deploy component through the Hetzner docker compose
project. By default this prints the command without running it; add --execute to
start the recalc.

Mode selection:
  --modes MODES           Game modes, comma-separated (default: 0,1,2,3)
                          0=std, 1=taiko, 2=catch, 3=mania
  --relax BITS            Relax bits, comma-separated (default: 0,1,2)
                          0=vanilla, 1=relax, 2=autopilot

Phase selection:
  --total-pp-only         Skip score PP recalculation; aggregate user totals only
  --no-total-pp           Skip user total PP aggregation; recalculate score PP only

Filters:
  --maps IDS              Filter to specific beatmap IDs, comma-separated
  --mods BITMASK          Filter to scores WITH these mods
  --no-mods BITMASK       Filter to scores WITHOUT these mods
  --mapper NAME           Filter by mapper name, fuzzy matched by the service
  --pp-zero               Only recalculate scores where pp is currently 0
  --after-date DATE       Only recalculate scores submitted on/after DATE
                          Format: YYYY-MM-DD, interpreted as UTC midnight
  --after-time TIMESTAMP  Only recalculate scores submitted on/after a Unix timestamp

Compose/runtime:
  --name NAME             Label used in the log filename
  --compose-dir DIR       Compose project directory (default: ${COMPOSE_DIR})
  --service SERVICE       Compose service to run (default: ${COMPOSE_SERVICE})
  --logs-dir DIR          Directory for --execute logs (default: ${LOGS_DIR})
  --pull                  Pull the compose service image before executing
  --execute               Run the command instead of only printing it

Examples:
  $0 --name test-beatmap-75 --maps 75 --modes 0 --relax 0
  $0 --name test-beatmap-75 --maps 75 --modes 0 --relax 0 --execute
  $0 --name full-recalc --pull --execute
  $0 --name reaggregate-only --total-pp-only --execute
  $0 --name std-dt-only --modes 0 --relax 0,1,2 --mods 64 --execute
  $0 --name recent-relax-0pp --modes 0,1,2 --relax 1 --pp-zero --after-date 2026-07-01 --execute

Common mod bitmasks:
  8=HD, 16=HR, 64=DT, 256=HT, 1024=FL
  72=HDDT, 80=HRDT, 88=HDHRDT, 832=DT+NC+HT
EOF
}

error() {
  echo "Error: $*" >&2
  exit 1
}

require_csv_int() {
  local name="$1"
  local value="$2"

  if [[ ! "$value" =~ ^[0-9]+(,[0-9]+)*$ ]]; then
    error "${name} must be a comma-separated list of integers"
  fi
}

require_int() {
  local name="$1"
  local value="$2"

  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    error "${name} must be an integer"
  fi
}

require_date() {
  local name="$1"
  local value="$2"

  if [[ ! "$value" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
    error "${name} must use YYYY-MM-DD format"
  fi
}

require_value() {
  local option="$1"
  local value="${2:-}"

  if [[ -z "$value" || "$value" == --* ]]; then
    error "${option} requires a value"
  fi
}

shell_quote() {
  printf "%q" "$1"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name)
      require_value "$1" "${2:-}"
      NAME="$2"
      shift 2
      ;;
    --modes)
      require_value "$1" "${2:-}"
      MODES="$2"
      shift 2
      ;;
    --relax)
      require_value "$1" "${2:-}"
      RELAX_BITS="$2"
      shift 2
      ;;
    --total-pp-only)
      TOTAL_PP_ONLY="1"
      shift
      ;;
    --no-total-pp)
      TOTAL_PP="0"
      shift
      ;;
    --maps)
      require_value "$1" "${2:-}"
      MAP_FILTER="$2"
      shift 2
      ;;
    --mods)
      require_value "$1" "${2:-}"
      MODS_FILTER="$2"
      shift 2
      ;;
    --no-mods)
      require_value "$1" "${2:-}"
      NEQ_MODS_FILTER="$2"
      shift 2
      ;;
    --mapper)
      require_value "$1" "${2:-}"
      MAPPER_FILTER="$2"
      shift 2
      ;;
    --pp-zero)
      PP_ZERO=1
      shift
      ;;
    --after-date)
      require_value "$1" "${2:-}"
      AFTER_DATE="$2"
      shift 2
      ;;
    --after-time)
      require_value "$1" "${2:-}"
      AFTER_TIME="$2"
      shift 2
      ;;
    --compose-dir)
      require_value "$1" "${2:-}"
      COMPOSE_DIR="$2"
      shift 2
      ;;
    --service)
      require_value "$1" "${2:-}"
      COMPOSE_SERVICE="$2"
      shift 2
      ;;
    --logs-dir)
      require_value "$1" "${2:-}"
      LOGS_DIR="$2"
      shift 2
      ;;
    --pull)
      PULL_IMAGE=1
      shift
      ;;
    --execute)
      EXECUTE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      error "unknown option: $1"
      ;;
  esac
done

require_csv_int "--modes" "$MODES"
require_csv_int "--relax" "$RELAX_BITS"

if [[ -n "$MAP_FILTER" ]]; then
  require_csv_int "--maps" "$MAP_FILTER"
fi

if [[ -n "$MODS_FILTER" ]]; then
  require_int "--mods" "$MODS_FILTER"
fi

if [[ -n "$NEQ_MODS_FILTER" ]]; then
  require_int "--no-mods" "$NEQ_MODS_FILTER"
fi

if [[ -n "$AFTER_DATE" ]]; then
  require_date "--after-date" "$AFTER_DATE"
fi

if [[ -n "$AFTER_TIME" ]]; then
  require_int "--after-time" "$AFTER_TIME"
fi

if [[ -n "$AFTER_DATE" && -n "$AFTER_TIME" ]]; then
  error "--after-date and --after-time cannot both be set"
fi

cmd=(
  docker compose run --rm --no-deps
  -e APP_COMPONENT=deploy
  -e APP_ENV=production
  -e DEPLOY_MODES="$MODES"
  -e DEPLOY_RELAX_BITS="$RELAX_BITS"
  -e DEPLOY_TOTAL_PP_ONLY="$TOTAL_PP_ONLY"
  -e DEPLOY_TOTAL_PP="$TOTAL_PP"
)

if [[ -n "$MAP_FILTER" ]]; then
  cmd+=(-e DEPLOY_MAP_FILTER="$MAP_FILTER")
fi

if [[ -n "$MODS_FILTER" ]]; then
  cmd+=(-e DEPLOY_MODS_FILTER="$MODS_FILTER")
fi

if [[ -n "$NEQ_MODS_FILTER" ]]; then
  cmd+=(-e DEPLOY_NEQ_MODS_FILTER="$NEQ_MODS_FILTER")
fi

if [[ -n "$MAPPER_FILTER" ]]; then
  cmd+=(-e DEPLOY_MAPPER_FILTER="$MAPPER_FILTER")
fi

if [[ "$PP_ZERO" -eq 1 ]]; then
  cmd+=(-e DEPLOY_PP_ZERO=1)
fi

if [[ -n "$AFTER_DATE" ]]; then
  cmd+=(-e DEPLOY_AFTER_DATE="$AFTER_DATE")
fi

if [[ -n "$AFTER_TIME" ]]; then
  cmd+=(-e DEPLOY_AFTER_TIME="$AFTER_TIME")
fi

cmd+=("$COMPOSE_SERVICE")

safe_name="$NAME"
if [[ -z "$safe_name" ]]; then
  safe_name="manual"
fi
safe_name="$(echo "$safe_name" | tr "[:upper:]" "[:lower:]" | sed "s/[^a-z0-9-]/-/g; s/^-*//; s/-*$//")"
if [[ -z "$safe_name" ]]; then
  safe_name="manual"
fi

timestamp="$(date +%Y%m%d-%H%M%S)"
log_file="${LOGS_DIR}/performance-service-recalc-${safe_name}-${timestamp}.log"

print_command() {
  echo "cd $(shell_quote "$COMPOSE_DIR")"

  if [[ "$PULL_IMAGE" -eq 1 ]]; then
    printf "docker compose pull %q\n" "$COMPOSE_SERVICE"
  fi

  printf "%q " "${cmd[@]}"
  printf "2>&1 | tee %q\n" "$log_file"
}

if [[ "$EXECUTE" -eq 0 ]]; then
  echo "Dry run. Re-run with --execute to start the recalc."
  echo
  print_command
  exit 0
fi

if [[ -z "${VAULT_TOKEN:-}" ]]; then
  error "VAULT_TOKEN is not set; docker compose needs it to pull performance-service secrets from Vault"
fi

if [[ ! -f "${COMPOSE_DIR}/docker-compose.yml" && ! -f "${COMPOSE_DIR}/compose.yml" ]]; then
  error "compose file not found in ${COMPOSE_DIR}; pass --compose-dir if the Hetzner compose project lives elsewhere"
fi

mkdir -p "$LOGS_DIR"
cd "$COMPOSE_DIR"

if [[ "$PULL_IMAGE" -eq 1 ]]; then
  docker compose pull "$COMPOSE_SERVICE"
fi

echo "Logging to ${log_file}"
"${cmd[@]}" 2>&1 | tee "$log_file"
