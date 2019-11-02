# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

cmake_minimum_required(VERSION 3.7)

# CMake include directories
set(
  CMAKE_MODULE_PATH
  "${CMAKE_CURRENT_SOURCE_DIR}/CMake"
  "${CMAKE_CURRENT_SOURCE_DIR}/build/fbcode_builder/CMake"
  ${CMAKE_MODULE_PATH}
)
set(CMAKE_INSTALL_DIR lib/cmake/mercurial)

# Find dependencies
set(CMAKE_THREAD_PREFER_PTHREAD ON)
set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

find_package(Gflags MODULE REQUIRED)
find_package(folly CONFIG REQUIRED)
find_package(Python COMPONENTS Interpreter)

include(RustStaticLibrary)
include(CMakePackageConfigHelpers)

add_subdirectory(lib)

# install module & CMake configs
install(
  EXPORT mercurial
  NAMESPACE Mercurial::
  FILE mercurial-targets.cmake
  DESTINATION "${CMAKE_INSTALL_DIR}"
)
configure_package_config_file(
  CMake/mercurial-config.cmake.in
  mercurial-config.cmake
  INSTALL_DESTINATION ${CMAKE_INSTALL_DIR}
  PATH_VARS
    CMAKE_INSTALL_DIR
)
install(
  FILES ${CMAKE_CURRENT_BINARY_DIR}/mercurial-config.cmake
  DESTINATION ${CMAKE_INSTALL_DIR}
)
