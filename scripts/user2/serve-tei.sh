#!/usr/bin/env bash
set -euo pipefail
echo "GPU-only pilot: the pinned CPU TEI image is disabled; use serve-st.sh with CUDA" >&2
exit 1
