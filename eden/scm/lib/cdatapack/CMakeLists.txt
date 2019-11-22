# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  eden_cdatapack
  STATIC
    cdatapack.c
)
target_link_libraries(
  eden_cdatapack
  PUBLIC
    eden_buffer
    LZ4::lz4
)
