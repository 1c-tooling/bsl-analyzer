#!/usr/bin/env bash
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
data="$root/experiments/user2-pilot"
pidfile="$data/serving/st.pid"
healthfile="$data/serving/st-health.json"
python="$data/model-env/.venv/bin/python"

if [[ -n "$(ss -ltn '( sport = :18881 )' | tail -n +2)" ]]; then
  echo "127.0.0.1:18881 is already in use" >&2
  exit 1
fi
mkdir -p "$data/serving" "$data/downloads/hf"
export HF_HOME="$data/downloads/hf"
"$python" -c 'import torch
if not torch.cuda.is_available(): raise SystemExit("GPU-only pilot: CUDA is unavailable")
torch.empty(1, device="cuda"); torch.cuda.synchronize()'
export USER2_DEVICE=cuda
printf '%s\n' "$USER2_DEVICE" > "$data/serving/device-check.json"

"$python" "$root/scripts/user2/serve.py" > "$data/serving/st.log" 2>&1 &
pid=$!
printf '%s\n' "$pid" > "$pidfile"
status=1
cleanup() {
  python3 - "$pidfile" "$pid" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
if path.exists() and path.read_text().strip() == sys.argv[2]:
    path.unlink()
PY
}
trap cleanup EXIT
for _ in $(seq 1 180); do
  if curl -fsS --max-time 2 http://127.0.0.1:18881/health > "$healthfile" 2>/dev/null; then
    echo "USER2 serving PID $pid at 127.0.0.1:18881"
    cat "$healthfile"
    break
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    set +e
    wait "$pid"
    status=$?
    set -e
    tail -20 "$data/serving/st.log" >&2
    exit "$status"
  fi
  sleep 2
done
if [[ ! -s "$healthfile" ]]; then
  echo "USER2 server did not become ready within 360 seconds" >&2
  exit 1
fi
set +e
wait "$pid"
status=$?
set -e
echo "USER2 server exited with status $status" >&2
tail -30 "$data/serving/st.log" >&2
exit "$status"
