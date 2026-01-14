# PP Recalculation K8s Jobs

This directory contains Kubernetes Job manifests for running PP recalculations.

## Quick Start

### Option 1: Use the generator script (recommended)

```bash
# Generate a job manifest
./generate-job.sh --name my-recalc --modes 0 --relax 0,1

# Apply it
kubectl apply -f recalc-job-my-recalc.yaml

# Monitor progress
kubectl logs -f job/performance-service-deploy-my-recalc

# Clean up when done
kubectl delete job performance-service-deploy-my-recalc
```

### Option 2: Copy and customize the template

```bash
cp deploy-template.yaml my-recalc.yaml
# Edit my-recalc.yaml to customize
kubectl apply -f my-recalc.yaml
```

### Option 3: Use a pre-made example

```bash
kubectl apply -f examples/full-recalc.yaml
```

## Generator Script Usage

```bash
./generate-job.sh [options]

Required:
  --name NAME           Job name suffix (e.g., "full-recalc", "test-single")

Mode Selection:
  --modes MODES         Game modes, comma-separated (default: 0,1,2,3)
                        0=std, 1=taiko, 2=catch, 3=mania
  --relax BITS          Relax bits, comma-separated (default: 0,1,2)
                        0=vanilla, 1=relax, 2=autopilot

Phase Selection:
  --total-pp-only       Skip Phase 1 (score recalc), only aggregate user totals
  --no-total-pp         Skip Phase 2 (user aggregation), only recalc scores

Filters:
  --maps IDS            Filter to specific beatmap IDs (comma-separated)
  --mods BITMASK        Filter to scores WITH these mods
  --no-mods BITMASK     Filter to scores WITHOUT these mods
  --mapper NAME         Filter by mapper name (fuzzy match)
```

## Common Examples

```bash
# Full server recalculation (all modes, all variants)
./generate-job.sh --name full-recalc

# Test with a single beatmap
./generate-job.sh --name test-beatmap-75 --maps 75 --modes 0 --relax 0,1

# Recalculate only osu!std (vanilla + relax + autopilot)
./generate-job.sh --name std-all --modes 0 --relax 0,1,2

# Recalculate only DT scores
./generate-job.sh --name dt-scores --mods 64

# Recalculate scores WITHOUT DT/NC/HT (no speed mods)
./generate-job.sh --name no-speed-mods --no-mods 832

# Re-aggregate user totals only (use existing score PP)
./generate-job.sh --name reaggregate --total-pp-only

# Recalculate specific beatmaps
./generate-job.sh --name specific-maps --maps 1808605,1821147,1844776

# Recalculate maps by a specific mapper
./generate-job.sh --name sotarks-maps --mapper Sotarks --modes 0
```

## Mod Bitmask Reference

| Mod | Value | Common Combos |
|-----|-------|---------------|
| NF | 1 | |
| EZ | 2 | |
| TD | 4 | |
| HD | 8 | |
| HR | 16 | HDHR = 24 |
| SD | 32 | |
| DT | 64 | HDDT = 72, HRDT = 80, HDDTHR = 88 |
| RX | 128 | |
| HT | 256 | |
| NC | 512 | (includes DT, so NC = 576) |
| FL | 1024 | HDFL = 1032 |
| AP | 8192 | |
| PF | 16384 | |

**Speed mods combined:** DT + NC + HT = 64 + 512 + 256 = 832

## Monitoring

```bash
# Watch job status
kubectl get jobs -w

# Stream logs
kubectl logs -f job/performance-service-deploy-JOBNAME

# Check for errors
kubectl describe job performance-service-deploy-JOBNAME
```

## Cleanup

Jobs are automatically cleaned up 24 hours after completion (via `ttlSecondsAfterFinished`).

To manually delete:
```bash
kubectl delete job performance-service-deploy-JOBNAME
```

To delete all completed recalc jobs:
```bash
kubectl delete jobs -l app=performance-service-deploy --field-selector=status.successful=1
```
