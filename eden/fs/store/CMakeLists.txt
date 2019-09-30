# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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
    eden_service_thrift_cpp
    eden_sqlite
    fb303::fb303
)

if (NOT WIN32)
  target_link_libraries(
    eden_store
    PUBLIC
      eden_rocksdb
  )
endif()

if("${EDEN_HAVE_EDENSCM}")
  add_subdirectory(hg)
endif()
if("${EDEN_HAVE_GIT}")
  add_subdirectory(git)
endif()

if (NOT WIN32)
  add_subdirectory(mononoke)
  add_subdirectory(test)
endif()
