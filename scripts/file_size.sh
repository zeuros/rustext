#!/usr/bin/env bash
# Fails when a source file has more than MAX_LINES lines before its `#[cfg(test)]` module.
# Files above the limit are pinned in file_size_baseline.txt at their current size: they may
# shrink, never grow, and leave the baseline once they fit.
# `scripts/file_size.sh --baseline` rewrites the baseline from the current sizes.
set -euo pipefail
cd "$(dirname "$0")/.."

readonly MAX_LINES=500
readonly BASELINE=scripts/file_size_baseline.txt

# Lines of a file before its first `#[cfg(test)]`, so inline tests don't count.
code_line_count() {
    awk '/^#\[cfg\(test\)\]/ { exit } { count++ } END { print count + 0 }' "$1"
}

oversized_files() {
    find crates -path '*/src/*' -name '*.rs' | sort | while read -r path; do
        local line_count
        line_count=$(code_line_count "$path")
        if ((line_count > MAX_LINES)); then
            echo "$line_count $path"
        fi
    done
}

if [[ "${1:-}" == "--baseline" ]]; then
    oversized_files >"$BASELINE"
    echo "wrote $BASELINE"
    exit 0
fi

failed=0
while read -r line_count path; do
    pinned_count=$(awk -v path="$path" '$2 == path { print $1 }' "$BASELINE")
    if [[ -z "$pinned_count" ]]; then
        echo "$path: $line_count lines, the limit is $MAX_LINES: split it"
        failed=1
    elif ((line_count > pinned_count)); then
        echo "$path: $line_count lines, grew past its baseline of $pinned_count: split it"
        failed=1
    fi
done < <(oversized_files)

while read -r pinned_count path; do
    if [[ ! -f "$path" ]] || (($(code_line_count "$path") <= MAX_LINES)); then
        echo "$path fits now: remove it from $BASELINE"
        failed=1
    elif (($(code_line_count "$path") < pinned_count)); then
        echo "$path shrank below its baseline of $pinned_count: run scripts/file_size.sh --baseline"
        failed=1
    fi
done <"$BASELINE"

exit "$failed"
