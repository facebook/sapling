#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -x
name="$1"
gh release view "$name" || gh release create --prerelease --title "$name" "$name"
