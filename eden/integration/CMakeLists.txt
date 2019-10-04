# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# TODO: I have only listed basic_test.py for now just to confirm that the
# test framework as a whole works in CMake-based builds.  We should eventually
# update this to include all test sources once we confirm that they all work.
file(GLOB INTEGRATION_SRCS "basic_test.py")
add_fb_python_unittest(
  integration_tests
  SOURCES ${INTEGRATION_SRCS}
  WORKING_DIRECTORY "${CMAKE_BINARY_DIR}"
  DEPENDS
    edenfsctl.main_lib
    eden_integration_lib
  ENV
    "CMAKE_SOURCE_DIR=${CMAKE_SOURCE_DIR}"
)

add_subdirectory(hg)
add_subdirectory(lib)
