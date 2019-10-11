#!/bin/sh
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

exec "$BINARY_HGPYTHON" "${RUN_TESTS_LIBRARY}/dummyssh" "$@"
