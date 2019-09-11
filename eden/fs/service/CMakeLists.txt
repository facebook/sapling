# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fbthrift_cpp_library(
  eden_service_thrift
  eden.thrift
  SERVICES
    EdenService
  DEPENDS
    eden_config_thrift
    fb303::fb303_thrift_cpp
)
add_fbthrift_cpp_library(
  streamingeden_thrift
  streamingeden.thrift
  SERVICES
    StreamingEdenService
  DEPENDS
    eden_service_thrift
    fb303::fb303_thrift_cpp
)

file(GLOB SERVICE_SRCS "*.cpp")

# A couple utility files our split out into a separate lower-level library
# This also effectively includes ThriftUtil.h, which has no corresponding .cpp
# file.
# TODO: In the future it would perhaps be nicer to move these to a separate
# eden/fs/thrift/ subdirectory, perhaps along with eden.thrift too.
list(
  REMOVE_ITEM SERVICE_SRCS
  ${CMAKE_CURRENT_SOURCE_DIR}/EdenError.cpp
  ${CMAKE_CURRENT_SOURCE_DIR}/PrettyPrinters.cpp
)
add_library(
  eden_service_thrift_util STATIC
  EdenError.cpp
  PrettyPrinters.cpp
)
target_link_libraries(
  eden_service_thrift_util
  PUBLIC
    eden_model
    Folly::folly
)

if (WIN32)
  list(
    REMOVE_ITEM SERVICE_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/EdenMain.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/StartupLogger.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/Systemd.cpp
  )
endif()

add_library(
  eden_service STATIC
  ${SERVICE_SRCS}
)
target_link_libraries(
  eden_service
  PUBLIC
    eden_service_thrift_util
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
  )
else()
  target_link_libraries(
    eden_service
    PUBLIC
      eden_fuse
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

target_include_directories(
  eden_service
  PUBLIC
    ${CPPTOML_INCLUDE_DIR}
)
