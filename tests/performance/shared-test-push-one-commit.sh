#!/bin/bash

set -e
set -o pipefail

if [[ -z "$REPO_PATH" ]]; then
  echo "Should be run by a test runner (manifold or fake)" >&2
  exit 1
fi

build_tools

setup_test_config_repo "$REPO_PATH" "$INSTANCE"

run_mononoke

echo "Setting up source repo"
cd "$REPO_PATH"
hginit_treemanifest "source"
cd "$REPO_PATH/source"
$REPOSYNTHESIZER --fill-existing-repo \
                 --path "$REPO_PATH/source" \
                 --seed 0 \
                 --commits-num 1 \
                 --non-ascii

echo "Pushing to Mononoke"
hgmn push --force
echo

cleanup
