#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

BUCK=buck2

if [[ "$1" == facebook/* ]]; then
    # Strip facebook prefix
    TARGET="/facebook:${1#*/}-manifest"
else
    TARGET=":$1-manifest"
fi

shift

set -x

$BUCK build "$@" //eden/mononoke/tests/integration:integration_runner_real "//eden/mononoke/tests/integration$TARGET" "//eden/mononoke/tests/integration${TARGET}[deps]"
