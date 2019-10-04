# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB INTEGRATION_LIB_SRCS "*.py")
add_fb_python_library(
  eden_integration_lib
  SOURCES ${INTEGRATION_LIB_SRCS}
  DEPENDS
    edenfsctl.main_lib
    eden_py
    eden_service_thrift_py
    eden_test_support
)
