# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fbthrift_library(
  eden_overlay_thrift
  overlay.thrift
  LANGUAGES cpp py
  PY_NAMESPACE facebook.eden.overlay
)

include(generated_targets.cmake)
eden_add_directory_library(eden_overlay_checker)
