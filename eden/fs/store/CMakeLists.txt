# Copyright (c) 2019-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

file(GLOB STORE_SRCS "*.cpp")
list(
  REMOVE_ITEM STORE_SRCS
  "${CMAKE_CURRENT_SOURCE_DIR}/eden_store_util.cpp"
)

if (WIN32)
  list(
    REMOVE_ITEM STORE_SRCS
      ${CMAKE_CURRENT_SOURCE_DIR}/BlobAccess.cpp
      ${CMAKE_CURRENT_SOURCE_DIR}/Diff.cpp
      ${CMAKE_CURRENT_SOURCE_DIR}/RocksDbLocalStore.cpp
  )
endif()

add_library(
  eden_store STATIC
    ${STORE_SRCS}
)
target_link_libraries(
  eden_store
  PUBLIC
    eden_config
    eden_model
    eden_model_git
    eden_service_thrift
    eden_sqlite
)

if (NOT WIN32)
  target_link_libraries(
    eden_store
    PUBLIC
      eden_rocksdb
  )
endif()

add_subdirectory(hg)

if (NOT WIN32)
  add_subdirectory(git)
  add_subdirectory(mononoke)
  add_subdirectory(test)
endif()
