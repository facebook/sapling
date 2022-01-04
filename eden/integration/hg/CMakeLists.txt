# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB HG_INTEGRATION_SRCS "*.py")
add_fb_python_unittest(
  hg_integration_tests
  SOURCES ${HG_INTEGRATION_SRCS}
  WORKING_DIRECTORY "${CMAKE_BINARY_DIR}"
  DEPENDS
    eden_hg_integration_lib
  ENV
    "CMAKE_SOURCE_DIR=${CMAKE_SOURCE_DIR}"
)

add_subdirectory(lib)
