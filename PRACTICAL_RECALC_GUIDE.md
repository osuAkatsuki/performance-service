# Practical PP Recalculation Guide

This guide covers how to run a full PP recalculation in production.

## Prerequisites

The `deploy` component needs network access to:
1. **MySQL database** - to read scores and write updated PP values
2. **Redis** - to update leaderboards and publish `peppy:update_cached_stats`
3. **beatmaps-service** - to fetch `.osu` files for PP calculation

## Where to Run

**Best option:** Run on the same server (or a server in the same network) where `performance-service` API is already running. It uses the same config, so you can reuse the `.env` file.

**Alternatively:** Any server that can reach the database, Redis, and beatmaps-service. Could be a dedicated worker box.

## Step-by-Step Instructions

### 1. SSH into your server and set up a persistent session

```bash
ssh your-server
tmux new -s pp-recalc    # or: screen -S pp-recalc
```

Using tmux/screen ensures the job continues running if your SSH connection drops.

### 2. Navigate to performance-service and ensure it's built

```bash
cd /path/to/performance-service
git pull                  # if needed
cargo build --release
```

### 3. Verify your `.env` is configured correctly

```bash
cat .env | grep -E "DATABASE|REDIS|BEATMAPS_SERVICE"
```

You should see output like:
```
DATABASE_HOST=localhost
DATABASE_PORT=3306
DATABASE_USERNAME=root
DATABASE_PASSWORD=...
DATABASE_NAME=akatsuki
REDIS_HOST=localhost
REDIS_PORT=6379
BEATMAPS_SERVICE_BASE_URL=http://localhost:8080
```

### 4. Do a dry run on a single beatmap first (recommended)

Test with one beatmap to verify everything is working:

```bash
DEPLOY_MODES=0 \
DEPLOY_RELAX_BITS=0 \
DEPLOY_MAP_FILTER=75 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release
```

### 5. Run the full recalculation

```bash
DEPLOY_MODES=0,1,2,3 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release 2>&1 | tee recalc-$(date +%Y%m%d-%H%M%S).log
```

This will:
- Recalculate PP for all scores across all modes (std, taiko, catch, mania)
- Recalculate PP for all variants (vanilla, relax, autopilot)
- Update user total PP and leaderboards
- Log output to both terminal and a timestamped file

### 6. Detach from tmux and let it run

```
Ctrl+B, then D
```

### 7. Re-attach later to check progress

```bash
tmux attach -t pp-recalc
```

Or tail the log file from another session:
```bash
tail -f recalc-*.log
```

## Timing Considerations

- **When to run:** During low-traffic hours (late night/early morning in your primary user timezone)
- **Duration:** Depends on score count. Could be hours for a large database.
- **Impact:** The recalc will hit beatmaps-service heavily. If that's the same service used for live requests, consider running off-peak.

## Monitoring Progress

The service logs progress every 100 beatmaps/users:

```
Beatmap recalculation progress: beatmaps_left=12345, mode=0, rx=0, beatmaps_processed=100
Processed users: users_left=5000, mode=0, rx=0, users_recalculated=1000
```

## Common Scenarios

### Full server recalculation (all modes, all variants)

```bash
DEPLOY_MODES=0,1,2,3 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release 2>&1 | tee recalc-full-$(date +%Y%m%d-%H%M%S).log
```

### Recalculate only osu!std (vanilla + relax + autopilot)

```bash
DEPLOY_MODES=0 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release 2>&1 | tee recalc-std-$(date +%Y%m%d-%H%M%S).log
```

### Recalculate only DT scores

```bash
DEPLOY_MODES=0,1,2,3 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_MODS_FILTER=64 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release 2>&1 | tee recalc-dt-$(date +%Y%m%d-%H%M%S).log
```

### Recalculate specific beatmaps only

```bash
DEPLOY_MODES=0 \
DEPLOY_RELAX_BITS=0,1 \
DEPLOY_MAP_FILTER=75,129891,1816113 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release
```

## Recovery: If Something Goes Wrong

### Crashed during Phase 2 (user aggregation)

If the recalc crashes during user total PP calculation, you can resume without re-doing Phase 1 (score PP):

```bash
DEPLOY_MODES=0,1,2,3 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_TOTAL_PP_ONLY=1 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release 2>&1 | tee recalc-resume-$(date +%Y%m%d-%H%M%S).log
```

This uses the existing score PP values and just re-aggregates user totals.

### Crashed during Phase 1 (score PP)

Unfortunately, there's no built-in resume for Phase 1. You'll need to restart from the beginning. The recalc is idempotent, so re-running is safe (just slower).

If you know approximately which beatmaps were already processed, you could use `DEPLOY_MAP_FILTER` to target only the remaining ones, but this requires manual tracking.

## Pre-flight Checklist

- [ ] Server has network access to MySQL, Redis, beatmaps-service
- [ ] `.env` file is configured with correct credentials
- [ ] Running in tmux/screen session
- [ ] Logging output to a file with `tee`
- [ ] Running during off-peak hours
- [ ] Tested on a small subset first (single beatmap)
- [ ] Notified team that recalc is running (if applicable)

## Reference: Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `DEPLOY_MODES` | Comma-separated game modes (0=std, 1=taiko, 2=catch, 3=mania) | `0,1,2,3` |
| `DEPLOY_RELAX_BITS` | Comma-separated variants (0=vanilla, 1=relax, 2=autopilot) | `0,1,2` |
| `DEPLOY_TOTAL_PP_ONLY` | `1` = skip score PP recalc, only aggregate user totals | `0` |
| `DEPLOY_TOTAL_PP` | `1` = run user total PP aggregation | `1` |
| `DEPLOY_MODS_FILTER` | Only scores WITH these mods (bitmask) | `64` (DT) |
| `DEPLOY_NEQ_MODS_FILTER` | Only scores WITHOUT these mods (bitmask) | `64` |
| `DEPLOY_MAPPER_FILTER` | Filter by mapper name (fuzzy match) | `Sotarks` |
| `DEPLOY_MAP_FILTER` | Comma-separated beatmap IDs | `75,129891` |

## Reference: Mod Bitmasks

| Mod | Value |
|-----|-------|
| NoFail | 1 |
| Easy | 2 |
| Hidden | 8 |
| HardRock | 16 |
| SuddenDeath | 32 |
| DoubleTime | 64 |
| Relax | 128 |
| HalfTime | 256 |
| Nightcore | 512 |
| Flashlight | 1024 |
| Autopilot | 8192 |
| Perfect | 16384 |
