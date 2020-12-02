#!/bin/bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#
# This is a helper tool to help you generate C interface from the Rust code
# using cbindgen. You will need to install cbindgen manually:
#
#   cargo install --force cbindgen

CONFIG="cbindgen.toml"
OUTPUT="c_api/RustEdenApi.h"

main() {
  cbindgen -vvv --config "$CONFIG" --output "$OUTPUT"
  python3 "$(hg root)/xplat/python/signedsource_lib/signedsource.py" sign "$OUTPUT"
}

main
