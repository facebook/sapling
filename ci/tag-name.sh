#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

DIR=$(dirname -- "$0")
VERSION=$(<"$DIR"/../SAPLING_VERSION)

# Create the commit info using either sl or git, whichever way we cloned this repo
if ! command -v sl &> /dev/null; then
  COMMIT_INFO=$(git -c "core.abbrev=8" show -s "--format=%cd+%h" "--date=format:%Y%m%d-%H%M%S")
else
  COMMIT_INFO=$(sl log --rev . --template '{date(date, "%Y%m%d-%H%M%S")}+{shortest(node, 8)}')
fi

echo "$VERSION"."$COMMIT_INFO"
