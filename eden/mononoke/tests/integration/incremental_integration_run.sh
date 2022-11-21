#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

NAME=$1
if [[ "$NAME" == facebook/* ]]; then
    SUBDIR="/facebook"
    # Strip facebook prefix
    NAME="${NAME#*/}"
else
    SUBDIR=""
fi
shift 1
fbsource=$(hg root)

set -x
"$fbsource/buck-out/v2/gen/fbcode/eden/mononoke/tests/integration/integration_runner_real.par" "$fbsource/buck-out/v2/gen/fbcode/eden/mononoke/tests/integration$SUBDIR/$NAME-manifest.json" "$@"
