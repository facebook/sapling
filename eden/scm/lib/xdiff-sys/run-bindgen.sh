#!/bin/sh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

SRC=../third-party/xdiff/xdiff.h

# Use allowlist to skip noise from stddef.h and stdint.h
bindgen \
  --allowlist-file $SRC \
  --with-derive-default \
  $SRC -o src/bindgen.rs
