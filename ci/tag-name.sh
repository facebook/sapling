#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

DIR=$(dirname -- "$0")
VERSION=$(<"$DIR"/../SAPLING_VERSION)
COMMIT_INFO=$(git -c "core.abbrev=8" show -s "--format=%cd-h%h" "--date=format:%Y%m%d-%H%M%S")
echo "$VERSION"."$COMMIT_INFO"
