#!/usr/bin/env bash
# Sprint_17 evidence in one command. Exit != 0 if any check fails.
# Needs: cargo-deny (`cargo install cargo-deny --locked`), python3. Development tools only.
set -euo pipefail
cd "$(dirname "$0")/.."
echo "== THIRD_PARTY_NOTICES.md vs Cargo.lock"
python3 scripts/gen_third_party_notices.py --check
echo "== cargo deny: licenses, bans, sources"
cargo deny check licenses bans sources
echo "== tests: PRIVACY.md single source, model licenses, notices header"
cargo test -p vad-ai -- privacy_tests model_manager --quiet
cargo test -p vad-app -- about theme --quiet
echo "OK (evidence only — see docs/Sprint_17/Sprint_Report_17.md for the human 🧑‍⚖️ list)"
