# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if(WIN32)
  file(GLOB INODES_SRCS "*.cpp" "sqliteoverlay/*.cpp")
  list(
    REMOVE_ITEM
    INODES_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/EdenDispatcher.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/OverlayFile.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/OverlayFileAccess.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/InodeMetadata.cpp
  )
  list(
    APPEND
    INODES_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/../fuse/InodeNumber.cpp
  )
else()
  file(GLOB INODES_SRCS "*.cpp")
endif()

add_library(
  eden_inodes STATIC
    ${INODES_SRCS}
)

target_link_libraries(
  eden_inodes
  PUBLIC
    eden_config
    eden_journal
    eden_model_git
    eden_overlay_thrift_cpp
    eden_service_thrift_util
    eden_sqlite
    eden_store
    eden_telemetry
    eden_utils
    Folly::folly
)

if (WIN32)
  target_link_libraries(
    eden_inodes
    PUBLIC
      eden_win
      eden_win_utils
  )
else()
  target_link_libraries(
    eden_inodes
    PUBLIC
      eden_fuse
      eden_overlay
      eden_service
      eden_takeover_thrift
  )
endif()

add_subdirectory(overlay)
add_subdirectory(test)
