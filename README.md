# performance-service

Multi-purpose microservice for all things performance, made for Akatsuki.

## Components

The service runs different components based on the `APP_COMPONENT` environment variable:

| Component | Description |
|-----------|-------------|
| `api` | REST API server for PP calculation (default, port 8665) |
| `deploy` | Production PP recalculation tool |
| `processor` | AMQP consumer for rework recalculation queue |
| `mass_recalc` | CLI to queue all users for rework recalculation |
| `individual_recalc` | CLI to queue a single user for rework recalculation |

## Building

```bash
cargo build --release
```

## Running

```bash
APP_COMPONENT=api cargo run --release
```

## Configuration

Copy `.env.example` to `.env` and configure:

```bash
# Service
APP_COMPONENT=api
API_HOST=127.0.0.1
API_PORT=8665

# Database
DATABASE_HOST=localhost
DATABASE_PORT=3306
DATABASE_USERNAME=root
DATABASE_PASSWORD=password
DATABASE_NAME=akatsuki
DATABASE_POOL_MAX_SIZE=16

# AMQP (RabbitMQ) - required for processor/mass_recalc/individual_recalc
AMQP_HOST=localhost
AMQP_PORT=5672
AMQP_USERNAME=guest
AMQP_PASSWORD=guest
AMQP_POOL_MAX_SIZE=10

# Redis
REDIS_HOST=localhost
REDIS_PORT=6379
REDIS_DATABASE=0

# External Services
BEATMAPS_SERVICE_BASE_URL=http://localhost:8080
SERVICE_READINESS_TIMEOUT=60
```

## Production PP Recalculation

The `deploy` component recalculates PP for all scores and updates user statistics. This uses the **same PP calculation algorithm** as live score submissions.

> **See [PRACTICAL_RECALC_GUIDE.md](PRACTICAL_RECALC_GUIDE.md) for step-by-step instructions on running a recalculation in production.**

### Game Modes

| Mode | Value | Relax Bit |
|------|-------|-----------|
| osu!std | 0 | 0 (vanilla), 1 (relax), 2 (autopilot) |
| osu!taiko | 1 | 0 (vanilla), 1 (relax) |
| osu!catch | 2 | 0 (vanilla), 1 (relax) |
| osu!mania | 3 | 0 (vanilla) |

### Interactive Mode

```bash
APP_COMPONENT=deploy cargo run --release
```

You'll be prompted for:
- **Modes**: Comma-separated list (e.g., `0,1,2,3`)
- **Relax bits**: Comma-separated list (e.g., `0,1,2`)
- **Total PP recalc only**: `y` to skip individual score recalc, `n` to recalc scores first
- **Total PP**: `y` to recalculate user total PP and leaderboards
- **Mod value recalc only**: Filter to scores with specific mods
- **Neq mod value recalc only**: Filter to scores WITHOUT specific mods
- **Mapper recalc only**: Filter to beatmaps by mapper name
- **Map recalc only**: Filter to specific beatmap IDs

### Environment Variable Mode

For automated/scripted recalculation:

```bash
# Full recalculation across all modes
DEPLOY_MODES=0,1,2,3 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release
```

### Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `DEPLOY_MODES` | Comma-separated game modes | `0,1,2,3` |
| `DEPLOY_RELAX_BITS` | Comma-separated relax bits | `0,1,2` |
| `DEPLOY_TOTAL_PP_ONLY` | Set to `1` to skip Phase 1 (score PP recalc) | `0` |
| `DEPLOY_TOTAL_PP` | Set to `1` to run Phase 2 (user total PP aggregation) | `1` |
| `DEPLOY_MODS_FILTER` | Only scores WITH these mods (bitmask) | `64` (DT) |
| `DEPLOY_NEQ_MODS_FILTER` | Only scores WITHOUT these mods | `64` |
| `DEPLOY_MAPPER_FILTER` | Filter by mapper name (fuzzy) | `Sotarks` |
| `DEPLOY_MAP_FILTER` | Comma-separated beatmap IDs | `123,456,789` |

### Common Recalculation Scenarios

**Full server recalculation (all modes, all variants):**
```bash
DEPLOY_MODES=0,1,2,3 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release
```

**Recalculate only osu!std (vanilla + relax + autopilot):**
```bash
DEPLOY_MODES=0 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release
```

**Recalculate only DT scores (all modes):**
```bash
DEPLOY_MODES=0,1,2,3 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_MODS_FILTER=64 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release
```

**Re-aggregate user totals only (skip Phase 1, use existing score PP):**
```bash
DEPLOY_MODES=0,1,2,3 \
DEPLOY_RELAX_BITS=0,1,2 \
DEPLOY_TOTAL_PP_ONLY=1 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release
```

**Recalculate specific beatmaps:**
```bash
DEPLOY_MODES=0 \
DEPLOY_RELAX_BITS=0,1 \
DEPLOY_MAP_FILTER=75,129891,1816113 \
DEPLOY_TOTAL_PP_ONLY=0 \
DEPLOY_TOTAL_PP=1 \
APP_COMPONENT=deploy cargo run --release
```

### Recalculation Phases

The deploy component runs in two phases, controlled by environment variables:

**Phase 1: Score PP Recalculation** (runs unless `DEPLOY_TOTAL_PP_ONLY=1`)
- Fetches the `.osu` file for each beatmap from beatmaps-service
- Re-runs the PP calculation algorithm on each individual score
- Updates the `pp` column in `scores`, `scores_relax`, or `scores_ap` tables
- This is the computationally expensive phase

**Phase 2: User Total PP Aggregation** (runs if `DEPLOY_TOTAL_PP=1`)
- Uses the existing `pp` values already in the score tables (does NOT recalculate them)
- Recalculates which score is "best" per beatmap (`completed=3` vs `completed=2`)
- Aggregates each user's top 100 scores using the weighted formula
- Updates `user_stats.pp`
- Updates Redis leaderboards (global and country)
- Publishes `peppy:update_cached_stats` for each user

**Common configurations:**

| TOTAL_PP_ONLY | TOTAL_PP | What happens |
|---------------|----------|--------------|
| `0` | `1` | Full recalc: Phase 1 then Phase 2 |
| `0` | `0` | Score PP only: Phase 1 only (no user totals) |
| `1` | `1` | Aggregation only: Phase 2 only (uses existing score PP) |

**When to use aggregation only (`DEPLOY_TOTAL_PP_ONLY=1`):**
- A previous recalc crashed during Phase 2 and you need to resume
- You fixed a bug in the aggregation/leaderboard logic (not the PP algorithm)
- You want to rebuild Redis leaderboards without changing score PP values

### Performance

- Beatmaps are processed with 10 concurrent tasks
- Users are processed in batches of 1000 with 100 concurrent tasks
- Progress is logged every 100 beatmaps/users

## Rework Recalculation

The `mass_recalc` and `processor` components are for testing **experimental PP algorithms** (reworks). These use different calculation formulas than live score submission.

### Available Reworks

| ID | Name | Description |
|----|------|-------------|
| 19 | improved_miss_penalty | Improved miss penalty formula |
| 21 | flashlight_hotfix | Flashlight mod adjustments |
| 22 | remove_accuracy_pp | Removes accuracy PP component |
| 23 | stream_nerf_speed_value | Nerfs stream speed value |
| 24 | remove_manual_adjustments | Removes manual map adjustments |
| 25 | fix_inconsistent_powers | Fixes inconsistent power calculations |
| 26 | aim_accuracy_fix | Aim and accuracy fixes |
| 27 | improved_miss_penalty_and_acc_rework | Combined miss penalty + accuracy rework |
| 28 | everything_at_once | All experimental changes combined |

### Running a Rework Recalculation

1. Start the processor (listens to AMQP queue):
   ```bash
   APP_COMPONENT=processor cargo run --release
   ```

2. Queue all users for a rework:
   ```bash
   APP_COMPONENT=mass_recalc cargo run --release
   # Enter the rework ID when prompted
   ```

   Or via environment:
   ```bash
   MASS_RECALC_REWORK_ID=19 APP_COMPONENT=mass_recalc cargo run --release
   ```

3. Queue a single user:
   ```bash
   APP_COMPONENT=individual_recalc cargo run --release
   # Enter user ID and rework ID when prompted
   ```

### Rework Data Storage

- `rework_scores` - Individual score PP calculations
- `rework_stats` - User total PP for each rework
- `rework_queue` - Processing queue status
- Redis: `rework:leaderboard:{rework_id}` - Rework leaderboards

## API Endpoints

### POST /api/v1/calculate

Calculate PP for one or more scores.

**Request:**
```json
[
  {
    "beatmap_id": 75,
    "beatmap_md5": "a5b99395a42bd55bc5eb1d2411cbdf8b",
    "mode": 0,
    "mods": 0,
    "max_combo": 314,
    "accuracy": 98.5,
    "miss_count": 1
  }
]
```

Or with hit counts instead of accuracy:
```json
[
  {
    "beatmap_id": 75,
    "beatmap_md5": "a5b99395a42bd55bc5eb1d2411cbdf8b",
    "mode": 0,
    "mods": 0,
    "max_combo": 314,
    "count_300": 200,
    "count_100": 10,
    "count_50": 2,
    "miss_count": 1
  }
]
```

**Response:**
```json
[
  {
    "stars": 5.23,
    "pp": 234.56,
    "ar": 9.0,
    "od": 8.0,
    "max_combo": 315
  }
]
```

## PP Calculation Algorithm

- **osu!std Relax**: Uses `akatsuki_pp_rs::osu_2019::OsuPP` (2019 algorithm)
- **All other modes**: Uses `rosu-pp` via `beatmap.performance()`

Total PP formula:
```
Total PP = SUM(score.pp * 0.95^index) + bonus_pp
Bonus PP = 416.6667 * (1 - 0.995^score_count)
```

Where `index` is the score's position when sorted by PP descending (top 100 scores only).
