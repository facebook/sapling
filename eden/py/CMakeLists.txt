# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB_RECURSE EDEN_PY_SRCS "eden/*.py")
add_fb_python_library(
  eden_py
  NAMESPACE ""
  SOURCES ${EDEN_PY_SRCS}
  DEPENDS eden_service_thrift_py
)
