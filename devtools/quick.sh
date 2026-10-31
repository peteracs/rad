#!/usr/bin/env bash
# Fast edit loop. Release builds use LTO and take minutes; dogfood programs run
# in milliseconds, so an unoptimized incremental build is the better trade.
#
#   devtools/quick.sh build                  rebuild target/debug/rad.exe
#   devtools/quick.sh test <filter>          run the rad-vm unit tests matching filter
#   devtools/quick.sh rad <args...>          rebuild, then run rad with args
#
# Run the full `cargo test --workspace` once, before committing.
set -euo pipefail
cd "$(dirname "$0")/.."

# Environment only: the checked-in profiles stay non-incremental because
# incremental state is what exhausts constrained developer drives.
export CARGO_PROFILE_DEV_INCREMENTAL=true
export CARGO_PROFILE_TEST_INCREMENTAL=true

build() {
    cargo build -p rad-cli --quiet
}

case "${1:-}" in
    build)
        build
        ;;
    test)
        shift
        cargo test -p rad-vm --lib --quiet "$@"
        ;;
    rad)
        shift
        build
        exec target/debug/rad "$@"
        ;;
    *)
        sed -n '2,9p' "$0"
        exit 2
        ;;
esac
