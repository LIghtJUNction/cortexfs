#!/usr/bin/env bash
set -euo pipefail

ctx status
ctx doctor || true
ctx agent status executor
ctx agent env executor
ctx agent tools executor
timeout 3 ctx ping agent/executor || true
