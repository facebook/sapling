# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB UTILS_SRCS "*.cpp")

if (WIN32)
  list(
    REMOVE_ITEM UTILS_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/FutureUnixSocket.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/IoFuture.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/SSLContext.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/Thread.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/UnixSocket.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/XAttr.cpp
  )
endif()

add_library(
  eden_utils STATIC
    ${UTILS_SRCS}
)

target_link_libraries(
  eden_utils
  PUBLIC
    eden_build_config
    eden_service_thrift_cpp
    Folly::folly
    edencommon::edencommon_utils
)

if (WIN32)
  target_link_libraries(
    eden_utils
    PUBLIC
      Userenv.lib
      Rpcrt4.lib
      dbghelp.lib
  )
endif()

add_subdirectory(test)
