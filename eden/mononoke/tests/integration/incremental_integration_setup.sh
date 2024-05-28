#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

BUCK=buck2

if [ $# -eq 0 ]; then
    bold=$(tput bold)
    normal=$(tput sgr0)
    >&2 echo " ${bold} No arguments provided, use one of: ${normal}"
    buck2 uquery "kind('sh_test', fbcode//eden/mononoke/tests/integration:)" --output-attribute name -v 0  2> /dev/null | jq 'to_entries[] | select(.key | contains("disable-all-network-access") | not) | .value.name'| tr '\n' ', ' | sed 's/,$//'
    printf "\n"
    exit 1
fi


if [[ "$1" == facebook/* ]]; then
    # Strip facebook prefix
    TARGET="/facebook:${1#*/}-manifest"
else
    TARGET=":$1-manifest"
fi

shift

# Set the default build mode to 'mode/dev-nosan-lg', which is the same mode used in CI.
build_args=("@fbcode//mode/dev-nosan-lg")
if [ $# -ne 0 ]; then
    build_args=("$@")
fi

set -x

$BUCK build "${build_args[@]}" //eden/mononoke/tests/integration:integration_runner_real "//eden/mononoke/tests/integration$TARGET" "//eden/mononoke/tests/integration${TARGET}[deps]"
