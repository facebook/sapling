# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_SRCS "*.cpp")
list(
  REMOVE_ITEM STORE_SRCS
  "${CMAKE_CURRENT_SOURCE_DIR}/eden_store_util.cpp"
  "${CMAKE_CURRENT_SOURCE_DIR}/BackingStoreType.cpp"
)

add_library(
  eden_store_type STATIC
    "${CMAKE_CURRENT_SOURCE_DIR}/BackingStoreType.cpp"
)
add_library(
  eden_store STATIC
    ${STORE_SRCS}
)

target_link_libraries(
  eden_store_type
  PUBLIC
    eden_utils
)
target_link_libraries(
  eden_store
  PUBLIC
    edencommon::edencommon_os
    eden_config
    eden_model
    eden_model_git
    eden_service_thrift_cpp
    eden_sqlite
    eden_store_type
    eden_store_filter
    eden_telemetry
    eden_utils
    fb303::fb303
    edencommon::edencommon_utils
)

target_link_libraries(
  eden_store
  PUBLIC
    eden_rocksdb
)

add_subdirectory(hg)
add_subdirectory(filter)

if("${EDEN_HAVE_GIT}")
  add_subdirectory(git)
endif()

add_subdirectory(test)
