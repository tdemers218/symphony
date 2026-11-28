#!/usr/bin/env bash
# Timing checks (ENGINEERING E6): release-build benchmarks that fail when
# something got clearly slower. Not part of check.sh (timings need a
# release build and a quiet-ish machine); run before merging work that
# touches drawing, erasing, stamping or export:  scripts/perf.sh
# The every-commit check that an unchanged frame rebuilds nothing is a
# normal test (an_unchanged_frame_rebuilds_nothing) and runs in check.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
export SYMPHONY_PERF_GATE=1
nice -n 10 cargo test -q --release -p symphony-vector stamped_cost -- --ignored --nocapture
nice -n 10 cargo test -q --release -p symphony-app erase_cost -- --ignored --nocapture --test-threads=1
nice -n 10 cargo test -q --release -p symphony-app chart_cost -- --ignored --nocapture --test-threads=1
echo "Timing checks passed."
