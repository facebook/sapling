#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Run production build for publishing to the vscode marketplace

# We only want to publish open source builds, not internal ones.
# Fail if we see facebook-only files in the repo.
if [ -f ./facebook/README.facebook.md ]; then
   echo "Facebook README exists. Make sure you only publish the vscode extension from the external repo."
   exit 1
fi

export NODE_ENV=production
echo "Building Extension"
webpack --config extension.webpack.config.ts
echo "Building Webview"
webpack --config webview.webpack.config.ts
echo "Build complete"
