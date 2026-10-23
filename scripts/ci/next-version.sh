#!/usr/bin/env bash
# Works out the release version for the commit being built (HEAD).
#
# Cargo.toml owns MAJOR.MINOR and the lowest PATCH; CI owns PATCH beyond it,
# so routine merges to main release 0.2.0, 0.2.1, 0.2.2 ... with no version
# commits. Bump MAJOR or MINOR in Cargo.toml to start a new line (0.3.0).
#
# - HEAD already carries a release tag (a re-run): reuse it, exists=true.
# - Otherwise: the next free PATCH in Cargo.toml's MAJOR.MINOR line, never
#   below Cargo.toml's PATCH, and never behind the latest release.
#
# Outputs (to $GITHUB_OUTPUT when set): version, tag, previous_tag, exists.
set -euo pipefail

base="$(sed -n 's/^version = "\([0-9]*\.[0-9]*\.[0-9]*\)"$/\1/p' Cargo.toml | head -n 1)"
if [[ -z "$base" ]]; then
  echo "Cargo.toml [package] version must be a plain MAJOR.MINOR.PATCH" >&2
  exit 1
fi
IFS=. read -r major minor patch <<<"$base"

# Release tags only (vMAJOR.MINOR.PATCH), oldest to newest.
releases="$(git tag --list 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V || true)"
latest="$(tail -n 1 <<<"$releases")"

existing="$(git tag --points-at HEAD --list 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n 1 || true)"
if [[ -n "$existing" ]]; then
  tag="$existing"
  version="${tag#v}"
  previous="$(grep -B 1 -x -F "$tag" <<<"$releases" | grep -v -x -F "$tag" || true)"
  exists=true
else
  last_patch="$(grep -E "^v${major}\.${minor}\.[0-9]+$" <<<"$releases" | sed 's/.*\.//' | sort -n | tail -n 1 || true)"
  if [[ -n "$last_patch" ]] && (( last_patch + 1 > patch )); then
    patch=$((last_patch + 1))
  fi
  version="${major}.${minor}.${patch}"
  tag="v${version}"
  previous="$latest"
  exists=false
  if [[ -n "$latest" && "$(printf '%s\n%s\n' "${latest#v}" "$version" | sort -V | tail -n 1)" != "$version" ]]; then
    echo "Cargo.toml version $base is behind the latest release $latest; raise MAJOR.MINOR in Cargo.toml." >&2
    exit 1
  fi
fi

echo "version=$version tag=$tag previous_tag=${previous:-none} exists=$exists"
if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "version=$version"
    echo "tag=$tag"
    echo "previous_tag=$previous"
    echo "exists=$exists"
  } >>"$GITHUB_OUTPUT"
fi
