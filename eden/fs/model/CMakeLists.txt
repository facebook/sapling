# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(generated_targets.cmake)
eden_add_directory_library(eden_model)

add_subdirectory(git)
if(BUILD_TESTS)
  add_subdirectory(test)
endif()
