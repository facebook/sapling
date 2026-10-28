# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(generated_targets.cmake)
eden_add_directory_library(eden_inodes)

add_subdirectory(overlay)
add_subdirectory(fscatalog)
add_subdirectory(memcatalog)
add_subdirectory(test)
