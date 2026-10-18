#!/usr/bin/env bash
# scripts/publish-db.sh - Package and publish rolling database release to GitHub
#
# Usage:
#   ./scripts/publish-db.sh [PATH_TO_GURD_DB]
#
# By default, uses ~/.local/share/gurd/gurd.db.

set -euo pipefail

DRY_RUN=false
DB_PATH="$HOME/.local/share/gurd/gurd.db"
REPO="sudomonas/gurd"
TAG="database"
DIST_DIR="dist/db"

while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        -h|--help)
            echo "Usage: ./scripts/publish-db.sh [--dry-run] [PATH_TO_GURD_DB]"
            exit 0
            ;;
        *)
            DB_PATH="$1"
            shift
            ;;
    esac
done

if [ ! -f "$DB_PATH" ]; then
    echo "Error: Database file not found at: $DB_PATH" >&2
    echo "Run 'gurd update' first to build your database." >&2
    exit 1
fi

echo "=================================================="
echo "          gurd Database Release Publisher         "
echo "=================================================="
echo "Source DB: $DB_PATH"
if [ "$DRY_RUN" = true ]; then
    echo "Mode:      DRY RUN (no files will be uploaded)"
fi

mkdir -p "$DIST_DIR"
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'gurd-pub-db')"
TMP_VACUUM="$TMP_DIR/gurd.db"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

echo ""
echo "1. Checking database integrity..."
INTEGRITY=$(sqlite3 "$DB_PATH" "PRAGMA integrity_check;")
if [ "$INTEGRITY" != "ok" ]; then
    echo "Error: Database integrity check failed: $INTEGRITY" >&2
    exit 1
fi
echo "   ✓ Integrity check: OK"

echo ""
echo "2. Compacting database (VACUUM INTO)..."
sqlite3 "$DB_PATH" "VACUUM INTO '$TMP_VACUUM';"
VACUUM_SIZE=$(stat -c%s "$TMP_VACUUM" 2>/dev/null || stat -f%z "$TMP_VACUUM")
if command -v numfmt >/dev/null 2>&1; then
    VACUUM_SIZE_FMT=$(numfmt --to=iec-i --suffix=B "$VACUUM_SIZE")
else
    VACUUM_SIZE_FMT="$VACUUM_SIZE bytes"
fi
echo "   ✓ Compacted DB size: $VACUUM_SIZE_FMT"

echo ""
echo "3. Querying installed sources and metadata..."
CONCEPTS_COUNT=$(sqlite3 "$TMP_VACUUM" "SELECT count(*) FROM concepts;")
SOURCES_JSON=$(sqlite3 "$TMP_VACUUM" "SELECT json_group_array(json_object('slug', slug, 'version', version, 'release_date', release_date)) FROM sources;")

echo "   ✓ Total concepts: $CONCEPTS_COUNT"
echo "   ✓ Installed sources:"
sqlite3 "$TMP_VACUUM" "SELECT '     - ' || slug || ': version ' || version || COALESCE(' (' || release_date || ')', '') FROM sources;"

echo ""
echo "4. Compressing to gurd.db.zip (maximum compression)..."
ZIP_DEST="$PWD/$DIST_DIR/gurd.db.zip"
rm -f "$ZIP_DEST"
(
    cd "$TMP_DIR"
    zip -9 -q "$ZIP_DEST" gurd.db
)
ZIP_SIZE=$(stat -c%s "$ZIP_DEST" 2>/dev/null || stat -f%z "$ZIP_DEST")
if command -v numfmt >/dev/null 2>&1; then
    ZIP_SIZE_FMT=$(numfmt --to=iec-i --suffix=B "$ZIP_SIZE")
else
    ZIP_SIZE_FMT="$ZIP_SIZE bytes"
fi
echo "   ✓ Compressed archive size: $ZIP_SIZE_FMT (from $VACUUM_SIZE_FMT)"

echo ""
echo "5. Generating SHA-256 and manifest.json..."
ZIP_SHA256=$(sha256sum "$ZIP_DEST" | awk '{print $1}')
echo "$ZIP_SHA256  gurd.db.zip" > "$DIST_DIR/gurd.db.zip.sha256"

DATE_VERSION=$(date -u +"%Y.%m.%d")
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

cat <<EOF > "$DIST_DIR/manifest.json"
{
  "version": "$DATE_VERSION",
  "built_at": "$TIMESTAMP",
  "schema_version": 2,
  "sha256": "$ZIP_SHA256",
  "archive": "gurd.db.zip",
  "archive_size": $ZIP_SIZE,
  "size": $VACUUM_SIZE,
  "concepts_count": $CONCEPTS_COUNT,
  "sources": $SOURCES_JSON
}
EOF
echo "   ✓ SHA-256: $ZIP_SHA256"
echo "   ✓ Manifest saved to $DIST_DIR/manifest.json"

echo ""
echo "6. Publishing to GitHub Release '$TAG'..."
if [ "$DRY_RUN" = true ]; then
    echo "   Dry-run mode: skipping upload."
    echo "   Packaged assets are ready in $DIST_DIR/:"
    echo "     - $DIST_DIR/gurd.db.zip"
    echo "     - $DIST_DIR/gurd.db.zip.sha256"
    echo "     - $DIST_DIR/manifest.json"
    echo ""
    echo "=================================================="
    echo "             Dry Run Complete!                    "
    echo "=================================================="
    exit 0
fi

if ! command -v gh >/dev/null 2>&1; then
    echo "Notice: 'gh' CLI is not found in PATH."
    echo "Assets are packaged in $DIST_DIR/ ready for manual upload:"
    echo "  - $DIST_DIR/gurd.db.zip"
    echo "  - $DIST_DIR/gurd.db.zip.sha256"
    echo "  - $DIST_DIR/manifest.json"
    exit 0
fi

# Create release if it doesn't already exist
if ! gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
    echo "   Creating release '$TAG'..."
    gh release create "$TAG" \
        --repo "$REPO" \
        --title "Rolling Database Release" \
        --notes "Rolling prebuilt drug database for gurd. Continuously updated by maintainer."
fi

echo "   Uploading assets (overwriting previous release files)..."
gh release upload "$TAG" \
    --repo "$REPO" \
    --clobber \
    "$ZIP_DEST" \
    "$DIST_DIR/gurd.db.zip.sha256" \
    "$DIST_DIR/manifest.json"

echo ""
echo "=================================================="
echo "             Publishing Complete!                 "
echo "=================================================="
echo "Live URL: https://github.com/$REPO/releases/tag/$TAG"
echo "End users can now receive this database update anytime via:"
echo "    gurd sync"
