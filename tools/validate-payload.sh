#!/usr/bin/env bash
# Build-time release gate; never executes an engine or changes the payload.
set -euo pipefail
[ "$#" -eq 1 ] || { echo "usage: validate-payload.sh <payload-dir>" >&2; exit 1; }
STABILIZER_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PAYLOAD_DIR="$(cd "$1" && pwd)"
[ -s "$PAYLOAD_DIR/profile.txt" ] || { echo "missing or empty profile.txt" >&2; exit 1; }

profile_arg_count=0
# Tokenize before expanding directories, like the app, so install paths with
# spaces are preserved inside each resulting argument. No shell evaluation.
while IFS= read -r profile_line || [ -n "$profile_line" ]; do
    profile_line="${profile_line%$'\r'}"
    profile_line="${profile_line#$'\xef\xbb\xbf'}"
    trimmed="${profile_line#"${profile_line%%[![:space:]]*}"}"
    case "$trimmed" in ''|\#*) continue ;; esac
    read -r -a profile_tokens <<< "$profile_line"
    for profile_token in "${profile_tokens[@]}"; do
        case "$profile_token" in
            --*) profile_arg_count=$((profile_arg_count+1)) ;;
            *) echo "unexpected profile token: $profile_token" >&2; exit 1 ;;
        esac
        profile_value="${profile_token#*=}"
        case "$profile_value" in
            '{BIN}'*) dependency="$PAYLOAD_DIR/bin/${profile_value#\{BIN\}}" ;;
            '{LISTS}'*) dependency="$PAYLOAD_DIR/lists/${profile_value#\{LISTS\}}" ;;
            *) continue ;;
        esac
        [ -f "$dependency" ] && [ -s "$dependency" ] || {
            echo "missing or empty profile dependency: $dependency" >&2
            exit 1
        }
    done
done < "$PAYLOAD_DIR/profile.txt"

[ "$profile_arg_count" -gt 0 ] || { echo "profile.txt has no engine arguments" >&2; exit 1; }

# This strategy is validated against the AXV reference backend. Updating
# the backend intentionally requires reviewing its compatibility and pins.
(cd "$PAYLOAD_DIR" && shasum -a 256 -c "$STABILIZER_ROOT/assets/backend-reference.sha256")
echo "Profile dependencies and reference backend verified."
