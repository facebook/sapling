# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fbthrift_library(
  eden_service_thrift
  eden.thrift
  LANGUAGES cpp py
  PY_NAMESPACE facebook.eden
  SERVICES
    EdenService
  DEPENDS
    eden_config_thrift
    fb303::fb303_thrift
)
add_fbthrift_library(
  streamingeden_thrift
  streamingeden.thrift
  LANGUAGES cpp
  SERVICES
    StreamingEdenService
  DEPENDS
    eden_service_thrift
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

list(APPEND EDEN_STORE_IMPLEMENTATIONS eden_store_hg)
if(EDEN_HAVE_GIT)
  list(APPEND EDEN_STORE_IMPLEMENTATIONS eden_store_git)
endif()

add_library(
  eden_service STATIC
  ${SERVICE_SRCS}
)
target_link_libraries(
  eden_service
  PUBLIC
    eden_service_thrift_util
    streamingeden_thrift_cpp
    eden_config
    eden_inodes
    eden_model
    eden_store
    eden_tracing
    ${EDEN_STORE_IMPLEMENTATIONS}
    ${YARPL_LIBRARIES}
    Folly::folly
    cpptoml
)

if (WIN32)
  target_link_libraries(
    eden_service
    PUBLIC
      eden_win
      eden_win_utils
  )
else()
  target_link_libraries(
    eden_service
    PUBLIC
      eden_fuse
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
