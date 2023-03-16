#!/bin/sh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

cd "$(dirname "$0")" || exit

# Tauri doesn't like being run with the node configured by our repo's .yarnrc
# Instead of yarn tauri dev, we have to run it from node_modules.
# Additionally, in dev mode, we also want to use a dev run of the ISL server.
# We run this by invoking run-proxy directly rather than `yarn serve`, since
# we're not in the isl-server directory.
# You can pass additional arguments to run-proxy, but not all of them make sense,
# such as --foreground.

./node_modules/.bin/tauri dev -- -- node ../../isl-server/dist/run-proxy.js --dev "$@"
