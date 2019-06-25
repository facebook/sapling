#!/bin/sh
# Copyright (c) 2019-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


exec "$BINARY_HGPYTHON" "${RUN_TESTS_LIBRARY}/dummyssh" "$@"
