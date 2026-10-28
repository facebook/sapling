# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(generated_targets.cmake)
eden_add_directory_library(eden_store)

add_subdirectory(sl)
add_subdirectory(filter)

if("${EDEN_HAVE_GIT}")
  add_subdirectory(git)
else()
  add_library(eden_fs_store_git INTERFACE)
endif()

add_subdirectory(test)
