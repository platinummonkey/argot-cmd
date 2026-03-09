#!/usr/bin/env bash
# bump-version.sh <major|minor|patch>
#
# Full sequenced release:
#   1. Bumps the workspace version and updates the argot-cmd-derive version
#      constraint in Cargo.toml if the major or minor component changed.
#   2. Commits and pushes.
#   3. Tags v{NEW}-derive and pushes → CI publishes argot-cmd-derive.
#   4. Polls crates.io until argot-cmd-derive {NEW} is indexed.
#   5. Tags v{NEW}-cmd and pushes → CI publishes argot-cmd + GitHub release.
#
# Recovery (tag-only, no version bump):
#   bump-version.sh tag <derive|cmd>
set -euo pipefail

BUMP="${1:-}"
CRATE="${2:-}"

# ── Argument validation ────────────────────────────────────────────────────────
if [[ -z "$BUMP" || ! "$BUMP" =~ ^(major|minor|patch|tag)$ ]]; then
    echo "Usage: $0 <major|minor|patch>"
    echo "       $0 tag <derive|cmd>   (recovery: tag at current version)"
    exit 1
fi

if [[ "$BUMP" == "tag" && ( -z "$CRATE" || ! "$CRATE" =~ ^(derive|cmd)$ ) ]]; then
    echo "Usage: $0 tag <derive|cmd>"
    exit 1
fi

# ── Helpers ────────────────────────────────────────────────────────────────────
push_tag() {
    local tag="$1"
    if git rev-parse "${tag}" >/dev/null 2>&1; then
        echo "error: tag ${tag} already exists"
        exit 1
    fi
    git tag -a "${tag}" -m "Release ${tag}"
    git push origin "${tag}"
    echo "Pushed tag ${tag}"
}

wait_for_crate() {
    local name="$1" version="$2"
    echo "Waiting for ${name} ${version} to be indexed on crates.io..."
    for i in $(seq 1 40); do
        STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
            "https://crates.io/api/v1/crates/${name}/${version}")
        if [[ "$STATUS" == "200" ]]; then
            echo "${name} ${version} is indexed."
            return
        fi
        echo "  attempt ${i}/40: not yet indexed (HTTP ${STATUS}), retrying in 30s..."
        sleep 30
    done
    echo "error: timed out waiting for ${name} ${version} to appear on crates.io"
    exit 1
}

# ── Ensure working tree is clean ──────────────────────────────────────────────
if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: working tree is dirty — commit or stash changes first"
    exit 1
fi

# ── Read current version ──────────────────────────────────────────────────────
CURRENT=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
IFS='.' read -r MAJOR MINOR PATCH_NUM <<< "$CURRENT"

# ── Recovery: tag-only mode ────────────────────────────────────────────────────
if [[ "$BUMP" == "tag" ]]; then
    push_tag "v${CURRENT}-${CRATE}"
    exit 0
fi

# ── Compute new version ────────────────────────────────────────────────────────
case "$BUMP" in
    major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH_NUM=0 ;;
    minor) MINOR=$((MINOR + 1)); PATCH_NUM=0 ;;
    patch) PATCH_NUM=$((PATCH_NUM + 1)) ;;
esac
NEW="${MAJOR}.${MINOR}.${PATCH_NUM}"

echo "Bumping ${CURRENT} → ${NEW} (${BUMP})"

# ── Update workspace version ───────────────────────────────────────────────────
perl -i -pe "s/^version = \"${CURRENT}\"/version = \"${NEW}\"/" Cargo.toml

COUNT=$(grep -c "^version = \"${NEW}\"" Cargo.toml)
if [[ "$COUNT" -ne 2 ]]; then
    echo "error: expected 2 version lines updated in Cargo.toml, got ${COUNT}"
    exit 1
fi

# ── Update argot-cmd-derive version constraint if major/minor changed ──────────
# Compute old and new Cargo semver constraints:
#   0.x.y → "0.x"  (patch-level compat within the minor)
#   ≥1.0.0 → "N"   (patch+minor compat within the major)
OLD_MAJOR=$(echo "$CURRENT" | cut -d. -f1)
OLD_MINOR=$(echo "$CURRENT" | cut -d. -f2)
if [[ "$OLD_MAJOR" == "0" ]]; then
    OLD_CONSTRAINT="0.${OLD_MINOR}"
else
    OLD_CONSTRAINT="${OLD_MAJOR}"
fi

if [[ "$MAJOR" == "0" ]]; then
    NEW_CONSTRAINT="0.${MINOR}"
else
    NEW_CONSTRAINT="${MAJOR}"
fi

if [[ "$OLD_CONSTRAINT" != "$NEW_CONSTRAINT" ]]; then
    echo "Updating argot-cmd-derive version constraint: \"${OLD_CONSTRAINT}\" → \"${NEW_CONSTRAINT}\""
    perl -i -pe "s/(argot-cmd-derive\s*=\s*\{[^}]*version\s*=\s*\")${OLD_CONSTRAINT}(\")/${1}${NEW_CONSTRAINT}${2}/" Cargo.toml
fi

# ── Update Cargo.lock ──────────────────────────────────────────────────────────
cargo update -p argot-cmd -p argot-cmd-derive 2>&1 | grep -v "^$" || true

# ── Commit and push ────────────────────────────────────────────────────────────
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to ${NEW}"
git push

# ── Tag derive → wait for indexing → tag cmd ──────────────────────────────────
push_tag "v${NEW}-derive"

wait_for_crate "argot-cmd-derive" "${NEW}"

push_tag "v${NEW}-cmd"

echo ""
echo "Done. Both crates released at ${NEW}."
