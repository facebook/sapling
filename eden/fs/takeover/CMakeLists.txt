# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fbthrift_cpp_library(
  eden_takeover_thrift
  takeover.thrift
)

include(generated_targets.cmake)
eden_add_directory_library(eden_takeover)

if(BUILD_TESTS)
  add_subdirectory(test)
endif()
