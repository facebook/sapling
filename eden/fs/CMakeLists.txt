# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

if(WIN32)
  add_executable(
    edenfs
    win/service/main.cpp
  )
else()
  add_executable(
    edenfs
    service/oss/main.cpp
  )
endif()

install(TARGETS edenfs RUNTIME DESTINATION bin)
target_include_directories(
  edenfs
  PUBLIC
    ${CMAKE_SOURCE_DIR}
    ${CMAKE_BINARY_DIR}
)
target_link_libraries(
  edenfs
    eden_service
    Folly::folly
)

if (WIN32)
  add_compile_definitions(_UNICODE UNICODE)
endif()

add_subdirectory(config)
add_subdirectory(inodes)
add_subdirectory(journal)
add_subdirectory(model)
add_subdirectory(service)
add_subdirectory(sqlite)
add_subdirectory(store)
add_subdirectory(tracing)
add_subdirectory(utils)

# Platform specific code
if (WIN32)
  add_subdirectory(win/mount)
  add_subdirectory(win/store)
  add_subdirectory(win/utils)
else()
  add_subdirectory(fuse)
  add_subdirectory(rocksdb)
  add_subdirectory(takeover)
  add_subdirectory(testharness)
endif()
