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

add_library(
  eden_overlay_checker
  STATIC
  ${CMAKE_CURRENT_SOURCE_DIR}/OverlayChecker.cpp
  ${CMAKE_CURRENT_SOURCE_DIR}/OverlayChecker.h
)

if (NOT WIN32)
  target_link_libraries(
    eden_overlay_checker
    PRIVATE
      eden_fscatalog
  )
endif()
