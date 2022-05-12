# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_executable(
  edenfs
  service/oss/main.cpp
  notifications/edenfs.exe.manifest
)

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

if(NOT WIN32)
  add_executable(
    edenfs_privhelper
    service/PrivHelperMain.cpp
  )
  install(TARGETS edenfs_privhelper RUNTIME DESTINATION bin)
  target_include_directories(
    edenfs_privhelper
    PUBLIC
      ${CMAKE_SOURCE_DIR}
      ${CMAKE_BINARY_DIR}
  )
  target_link_libraries(
    edenfs_privhelper
      eden_fuse_privhelper
      Folly::folly
  )
endif()

add_library(
  eden_build_config STATIC
  eden-config.cpp
)

if (WIN32)
  add_compile_definitions(_UNICODE UNICODE)
endif()

add_subdirectory(config)
add_subdirectory(fuse)
add_subdirectory(inodes)
add_subdirectory(journal)
add_subdirectory(model)
add_subdirectory(rocksdb)
add_subdirectory(service)
add_subdirectory(sqlite)
add_subdirectory(store)
add_subdirectory(telemetry)
add_subdirectory(utils)
add_subdirectory(testharness)
add_subdirectory(py)
add_subdirectory(cli)
add_subdirectory(cli_rs/edenfsctl)
add_subdirectory(notifications)
add_subdirectory(takeover)
add_subdirectory(prjfs)
add_subdirectory(nfs)
