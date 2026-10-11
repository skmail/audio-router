#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
exec ./build.sh --open
