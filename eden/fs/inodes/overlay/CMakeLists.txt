# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fbthrift_library(
  eden_overlay_thrift
  overlay.thrift
  LANGUAGES cpp
)

if (EDEN_HAVE_THRIFT_PYTHON)
  add_fbthrift_python_library(
    eden_overlay_thrift_python
    overlay.thrift
    NAMESPACE facebook.eden
  )
endif()

include(generated_targets.cmake)
