# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB TEST_SUPPORT_SRCS "*.py")
add_fb_python_library(
  eden_test_support
  SOURCES ${TEST_SUPPORT_SRCS}
)
