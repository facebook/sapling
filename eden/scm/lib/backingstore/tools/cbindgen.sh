#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#
# This is a helper tool to help you generate C interface from the Rust code
# using cbindgen. You will need to install cbindgen manually:
#
#   cargo install --force cbindgen

cd "$(dirname "$0")"/..

set -e

CONFIG="cbindgen.toml"
OUTPUT="c_api/RustBackingStore.h"

main() {
  cbindgen --config "$CONFIG" --output "$OUTPUT"
  # There is no way to customize the return type for functions with cbindgen,
  # and we don't want cbindgen to generate these template types since MSVC does
  # not like these. So we use `sed` to strip these templates.
  #
  # Note: `-i"" -e` for BSD compatibility.
  sed -i"" -e "s/CFallibleBase<.*>/CFallibleBase/g"  "$OUTPUT"
  python3 "$(hg root)/xplat/python/signedsource_lib/signedsource.py" sign "$OUTPUT"
}

main
