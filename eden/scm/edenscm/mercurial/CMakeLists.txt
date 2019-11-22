# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  eden_mpatch
  STATIC
    mpatch.c
)
target_include_directories(
  eden_mpatch
  PUBLIC
    "${CMAKE_SOURCE_DIR}/eden/scm"
)
