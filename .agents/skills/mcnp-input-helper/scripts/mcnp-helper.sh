#!/usr/bin/env bash
set -euo pipefail

# Resolve repository root relative to this script (.agents/skills/mcnp-input-helper/scripts/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"

RELEASE_BIN="${REPO_ROOT}/target/release/mcnp-input-parser"
DEBUG_BIN="${REPO_ROOT}/target/debug/mcnp-input-parser"

if [[ -x "${RELEASE_BIN}" ]]; then
    exec "${RELEASE_BIN}" "$@"
elif [[ -x "${DEBUG_BIN}" ]]; then
    exec "${DEBUG_BIN}" "$@"
else
    cargo build --release --quiet --manifest-path "${REPO_ROOT}/Cargo.toml"
    exec "${RELEASE_BIN}" "$@"
fi
