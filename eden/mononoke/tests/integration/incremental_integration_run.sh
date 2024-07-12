#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

if [ $# -eq 0 ]; then
    bold=$(tput bold)
    normal=$(tput sgr0)
    >&2 echo " ${bold} No arguments provided, make sure you first run incremental_integration_setup.sh with one of: ${normal}"
    buck2 uquery "kind('sh_test', fbcode//eden/mononoke/tests/integration:)" --output-attribute name -v 0  2> /dev/null | jq 'to_entries[] | select(.key | contains("disable-all-network-access") | not) | .value.name'| tr '\n' ', ' | sed 's/,$//'
    printf "\n"
    exit 1
fi

NAME="$1"
if [[ "$NAME" == */* ]]; then
    SUBDIR="/$(dirname "$NAME")"
    # Strip facebook prefix
    NAME="$(basename "$NAME")"
else
    SUBDIR=""
fi
shift 1
fbsource=$(hg root)

set -x
"$fbsource/buck-out/v2/gen/fbcode/eden/mononoke/tests/integration/integration_runner_real.par" "$fbsource/buck-out/v2/gen/fbcode/eden/mononoke/tests/integration$SUBDIR/$NAME-manifest.json" "$@"
