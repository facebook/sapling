#!/bin/bash
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

ALL_EDENFS_MOUNTS=$(grep -e '^edenfs' /proc/mounts | awk '{print $2}')
for n in $ALL_EDENFS_MOUNTS; do
    # Sometimes, lazy unmounting causes other mounts to unmount.
    sudo umount -v -l "$n"
done

ALL_EDENFS_MOUNTS=$(grep -e '^edenfs' /proc/mounts | awk '{print $2}')
for n in $ALL_EDENFS_MOUNTS; do
    sudo umount -v -f "%f"
done
