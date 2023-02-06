#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# This file removes all internal-only files and deletes @fb-only commented lines from code
# This is only used in our internal CI tests, the actual removing when syncing to GitHub is done by shipit.

cd "$(dirname "$0")" || exit

function findAllSourceFiles() {
  find . \
    \( -name '*.ts' -o -name '*.tsx' -o -name '*.js' \) \
    -type f \
    \( -not -path '*/node_modules/*' -not -path '*/dist/*' -not -path '*/build/*' -not -path '.vscode-build' -not -path '*/coverage/*' \) \
    -print0
}

echo "replacing all fb-only comments..."

fbOnlyRegex='s/^([[:space:]]*).* \/\/ @fb-only/\1\/\/ @fb-only/g'
OS=$(uname)
if [ "$OS" = 'Darwin' ]; then
  # macOS ships with BSD version of sed, where -i '' is required to not create backup files,
  # but -i '' doesn't work on linux.
  findAllSourceFiles | xargs -0 sed -i '' -r "$fbOnlyRegex"
else
  findAllSourceFiles | xargs -0 sed -i'' -r "$fbOnlyRegex"
fi

echo "deleting all facebook/ directories..."
find . -path '*/facebook' -type d -print0 | xargs -0 rm -rf
