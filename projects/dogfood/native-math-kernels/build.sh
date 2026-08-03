#!/usr/bin/env sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
profile=${1:-release}
case "$profile" in
  debug) cargo build --manifest-path "$project_dir/Cargo.toml" ;;
  release) cargo build --release --manifest-path "$project_dir/Cargo.toml" ;;
  *) echo "profile must be debug or release" >&2; exit 2 ;;
esac

case "$(uname -s)" in
  Darwin)
    extension=librad_dogfood_math_kernels.dylib
    suffix=dylib
    ;;
  *)
    extension=librad_dogfood_math_kernels.so
    suffix=so
    ;;
esac
mkdir -p "$project_dir/out"
cp "$project_dir/target/$profile/$extension" "$project_dir/out/rad_dogfood_math_kernels"
cp "$project_dir/target/$profile/$extension" "$project_dir/out/rad_dogfood_math_kernels.$suffix"
printf '%s\n' "$project_dir/out/rad_dogfood_math_kernels"
