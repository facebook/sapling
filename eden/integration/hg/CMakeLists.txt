# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# TODO: Eventually we should support all tests here.
# At the moment I have only confirmed that the tests in status_test.py pass.
# file(GLOB HG_INTEGRATION_SRCS "*.py")
set(HG_INTEGRATION_SRCS "status_test.py")

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
