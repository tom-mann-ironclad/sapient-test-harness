#!/usr/bin/env bash
# Writes the CycloneDX SBOM for the shipped `sapient-harness` binary to
# <out-dir>/sapient-test-harness-cli.cdx.xml. Used by CI (as a check that
# generation works) and by release.yml (for the published SBOM).
#
# `--target all` lists the dependencies of every release platform, not just
# the machine running this (e.g. Windows-only crates). cargo-cyclonedx writes
# one SBOM per workspace crate; only the CLI's describes the shipped binary,
# so the others are removed.

set -euo pipefail

out_dir="${1:?usage: generate-sbom.sh <out-dir>}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo cyclonedx -v \
  --manifest-path "$repo_root/crates/sapient-test-harness-cli/Cargo.toml" \
  --format xml --target all --spec-version 1.5

mkdir -p "$out_dir"
mv "$repo_root/crates/sapient-test-harness-cli/sapient-test-harness-cli.cdx.xml" "$out_dir/"
find "$repo_root/crates" -name '*.cdx.xml' -delete
