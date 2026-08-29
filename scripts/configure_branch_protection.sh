#!/usr/bin/env bash
set -euo pipefail

repository=${1:-nickderaj/nswarm}
branch=${2:-main}

gh api --method PUT "repos/${repository}/branches/${branch}/protection" \
  --input scripts/branch-protection.json
