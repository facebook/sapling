#!/usr/bin/env bash
# Copyright (c) 2019-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


# This script is provided as a compatibility shim for those who want to run
# individual tests, using Buck:
#
#   buck run scm/mononoke/tests/integration:integration_runner -- TEST

d="$BUCK_DEFAULT_RUNTIME_RESOURCES"
exec "${d}/integration_runner_real" "${d}/manifest.json" "$@"
