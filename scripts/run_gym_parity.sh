#!/usr/bin/env bash
set -euo pipefail

exec cargo test --locked -p gym-bot --test parity --test parity_corpus --test service --test health -- "$@"
