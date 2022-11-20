#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -xe

# Treat the wasm file as a static asset so it can be "fetch()"-ed
# client-side. See also why we use '-t web' below.
PACK_OUT_DIR=../../static/wasm

[[ -d $PACK_OUT_DIR ]] && rm -rf $PACK_OUT_DIR

# Use '-t web' instead of '-t bundler' for server compatibility.
# Some servers (ex. internaldocs) use application/octet-stream not
# application/wasm for .wasm files which breaks native client-side
# import. '-t web' generates code with WebAssembly.instantiate
# fallback to be compatible with it [1].
# [1]: https://github.com/rustwasm/wasm-bindgen/blob/564ce74168904e95a7905a828488ec3029bcaad4/crates/cli-support/src/js/mod.rs#L799
wasm-pack build -t web -d $PACK_OUT_DIR
rm $PACK_OUT_DIR/README.md $PACK_OUT_DIR/package.json $PACK_OUT_DIR/wasm_bindings_bg.wasm.d.ts

# wasm-pack can run wasm-opt, but it is buggy [2] and wasm-opt bundled by
# wasm-pack is outdated (slow, 30s vs 1s). So we disabled wasm-opt feature
# of wasm-pack in Cargo.toml. However wasm-opt is useful. It can shrink
# the binary from 1.1MB to 0.7MB.
# Assume wasm-opt is in PATH and is newer. wasm-opt can be built from
# source [3].
# [2]: https://github.com/rustwasm/wasm-pack/issues/1190
# [3]: https://github.com/WebAssembly/binaryen
wasm-opt $PACK_OUT_DIR/wasm_bindings_bg.wasm -o $PACK_OUT_DIR/wasm_bindings_bg.wasm.opt -Os
mv $PACK_OUT_DIR/wasm_bindings_bg.wasm.opt $PACK_OUT_DIR/wasm_bindings_bg.wasm

# See src/utils/importBindings.js for client-side logic to import the module.
