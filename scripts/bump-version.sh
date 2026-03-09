#!/usr/bin/env bash
# bump-version.sh <major|minor|patch>
#
# Bumps the workspace version in Cargo.toml, updates Cargo.lock,
# commits, pushes, and creates + pushes a semver git tag.
set -euo pipefail

BUMP="${1:-}"

if [[ -z "$BUMP" || ! "$BUMP" =~ ^(major|minor|patch)$ ]]; then
    echo "Usage: $0 <major|minor|patch>"
    exit 1
fi

# ── Ensure working tree is clean ──────────────────────────────────────────────
if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: working tree is dirty — commit or stash changes first"
    exit 1
fi

# ── Read current version ──────────────────────────────────────────────────────
CURRENT=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

# ── Compute new version ───────────────────────────────────────────────────────
case "$BUMP" in
    major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
    minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
    patch) PATCH=$((PATCH + 1)) ;;
esac
NEW="${MAJOR}.${MINOR}.${PATCH}"

echo "Bumping ${CURRENT} → ${NEW} (${BUMP})"

# ── Update Cargo.toml (workspace + package sections both use bare version) ────
# Use perl for portability across macOS (BSD sed) and Linux (GNU sed).
perl -i -pe "s/^version = \"${CURRENT}\"/version = \"${NEW}\"/" Cargo.toml

# Verify both occurrences were updated
COUNT=$(grep -c "^version = \"${NEW}\"" Cargo.toml)
if [[ "$COUNT" -ne 2 ]]; then
    echo "error: expected 2 version lines updated in Cargo.toml, got ${COUNT}"
    exit 1
fi

# ── Update Cargo.lock ─────────────────────────────────────────────────────────
cargo update -p argot -p argot-derive 2>&1 | grep -v "^$" || true

# ── Commit and push ───────────────────────────────────────────────────────────
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to ${NEW}"
git push

# ── Tag and push ──────────────────────────────────────────────────────────────
TAG="v${NEW}"
git tag -a "${TAG}" -m "Release ${TAG}"
git push origin "${TAG}"

echo ""
echo "Released ${TAG}"
