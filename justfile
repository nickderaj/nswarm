set shell := ["bash", "-euo", "pipefail", "-c"]

# Deterministic pull-request gate. Requires the pinned tools in docs/DEVELOPMENT.md.
ci:
    ./scripts/ci.sh

# Merge-queue gate; Linux nightly, Miri, sanitizer, mutation, and fuzz prerequisites apply.
ci-full:
    ./scripts/ci.sh
    ./scripts/ci-full.sh

# Regenerate repository-owned artifacts into the checked-in destination.
generate:
    ./scripts/generate.sh generated
