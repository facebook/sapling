# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if(WIN32)
  file(GLOB INODES_SRCS "*.cpp")
  list(
    REMOVE_ITEM
    INODES_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/OverlayFile.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/OverlayFileAccess.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/InodeMetadata.cpp
  )
else()
  file(GLOB INODES_SRCS "*.cpp")
endif()

list(
  REMOVE_ITEM
  INODES_SRCS
  ${CMAKE_CURRENT_SOURCE_DIR}/RequestContext.cpp
  ${CMAKE_CURRENT_SOURCE_DIR}/InodeNumber.cpp
  ${CMAKE_CURRENT_SOURCE_DIR}/InodeMetadata.cpp
  ${CMAKE_CURRENT_SOURCE_DIR}/InodeTimestamps.cpp
)

add_library(
  eden_inodes_inodenumber STATIC
    ${CMAKE_CURRENT_SOURCE_DIR}/InodeNumber.cpp
)

target_link_libraries(
  eden_inodes_inodenumber
  PUBLIC
    Folly::folly
)

add_library(
  eden_inodes STATIC
    ${INODES_SRCS}
)

target_link_libraries(
  eden_inodes
  PUBLIC
    eden_config
    eden_inodes_inodenumber
    eden_inode_metadata
    eden_journal
    eden_model_git
    eden_nfs_dispatcher
    eden_nfs_nfsd3
    eden_overlay_thrift_cpp
    eden_service_thrift_util
    eden_sqlite
    eden_store
    eden_takeover_thrift
    eden_telemetry
    eden_utils
    eden_tree_overlay
    Folly::folly
    edencommon::edencommon_utils
  PRIVATE
    eden_nfs_server
)

if (WIN32)
  target_link_libraries(
    eden_inodes
    PUBLIC
      eden_prjfs
  )
else()
  target_link_libraries(
    eden_inodes
    PUBLIC
      eden_fuse
      eden_fscatalog
      eden_service
  )
endif()

add_library(
  eden_request_context STATIC
  RequestContext.cpp
)

target_link_libraries(
  eden_request_context
  PUBLIC
    eden_notifications
    eden_store
    eden_telemetry
    eden_utils
    Folly::folly
)

add_library(
  eden_inode_metadata STATIC
    InodeMetadata.cpp InodeTimestamps.cpp InodeMetadata.h InodeTimestamps.h
)

target_link_libraries(
  eden_inode_metadata
  PUBLIC
    eden_utils
  PRIVATE
    eden_fuse
    eden_utils
    Folly::folly
)

add_subdirectory(overlay)
add_subdirectory(fscatalog)
add_subdirectory(treeoverlay)
add_subdirectory(test)
