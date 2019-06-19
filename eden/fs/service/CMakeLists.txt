# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_thrift_cpp2_library(
  eden_service_thrift
  eden.thrift
  SERVICES
    EdenService
  DEPENDS
    eden_config_thrift
    fb303_thrift_cpp2
)
add_thrift_cpp2_library(
  streamingeden_thrift
  streamingeden.thrift
  SERVICES
    StreamingEdenService
  DEPENDS
    eden_service_thrift
)

file(GLOB SERVICE_SRCS "*.cpp")

if (WIN32)
  list(
    REMOVE_ITEM SERVICE_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/EdenInit.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/EdenMain.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/PrettyPrinters.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/StartupLogger.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/Systemd.cpp
  )
  add_library(
    eden_service STATIC
      ${SERVICE_SRCS}
  )
else()
  add_library(
    eden_service STATIC
      ${SERVICE_SRCS}
      oss/main.cpp
  )
endif()

target_link_libraries(
  eden_service
  PUBLIC
    eden_service_thrift
    streamingeden_thrift
    eden_config
    eden_inodes
    eden_model
    eden_store_hg
    eden_tracing
    ${YARPL_LIBRARIES}
    Folly::folly
)

if (WIN32)
  target_link_libraries(
    eden_service
    PUBLIC
      eden_win_mount
      eden_win_store
      eden_win_utils
      common_stats
  )
else()
  target_link_libraries(
    eden_service
    PUBLIC
      eden_fuse
      eden_model_git
      eden_store_git
      eden_takeover
  )
endif()

if (CURL_FOUND)
  target_link_libraries(
    eden_service
    PUBLIC
      curl
  )
endif()

if (APPLE)
  target_link_libraries(
    eden_service
    PUBLIC
    "-framework CoreFoundation"
    "-framework IOKit"
  )
endif()
