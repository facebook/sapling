#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

DIR=$(dirname -- "$0")
VERSION=$(<"$DIR"/../SAPLING_VERSION)

# Create the commit info using either sl or git, whichever way we cloned this repo
COMMIT_INFO=$(sl log --rev . --template '{date(date, "%Y%m%d-%H%M%S")}-h{shortest(node, 8)}') \
  || $(git -c "core.abbrev=8" show -s "--format=%cd-h%h" "--date=format:%Y%m%d-%H%M%S")

echo "$VERSION"."$COMMIT_INFO"
