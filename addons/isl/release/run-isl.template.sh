#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
# Facilitate redefining NODE in terms of SCRIPT_DIR via a regex.
NODE=node

if [ ! -x "$(command -v $NODE)" ]; then
  # shellcheck disable=SC2016
  echo 'ERROR: `node` is required to run Interactive Smartlog, but it was not'
  # shellcheck disable=SC2016
  echo 'found on the $PATH. For information on installing Node.js, see:'
  echo 'https://nodejs.dev/en/learn/how-to-install-nodejs/'
  exit 1
fi

"$NODE" "$SCRIPT_DIR/isl-server/dist/run-proxy.js" "$@"
