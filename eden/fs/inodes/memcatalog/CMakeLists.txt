# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  eden_mem_catalog STATIC
    MemInodeCatalog.cpp
)

target_link_libraries(
  eden_mem_catalog
  PUBLIC
    eden_utils
)
