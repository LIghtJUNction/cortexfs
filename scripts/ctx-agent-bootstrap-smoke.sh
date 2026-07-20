#!/usr/bin/env bash
set -euo pipefail

ctx status
ctx doctor || true
ctx agent status coder
ctx agent env coder
ctx agent tools coder
timeout 3 ctx ping agent/coder || true
