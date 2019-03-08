# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.
#
# Find libgit2
#
# This package sets:
# LIBGIT2_FOUND - Whether libgit2 was found
# LIBGIT2_INCLUDE_DIR - The include directory for libgit2
# LIBGIT2_LIBRARY - The libgit2 library
# 
include(FindPackageHandleStandardArgs)
find_package(PkgConfig)

pkg_check_modules(LIBGIT2 libgit2 QUIET)

if(LIBGIT2_FOUND)
  set(CMAKE_IMPORT_FILE_VERSION 1)
  add_library(libgit2 INTERFACE)
  target_compile_options(libgit2 INTERFACE "${LIBGIT2_CFLAGS_OTHER}")
  target_compile_options(
    libgit2 INTERFACE
    "${LIBGIT2_CFLAGS_OTHER}"
  )
  target_include_directories(
    libgit2 INTERFACE
    "${LIBGIT2_INCLUDE_DIR}"
  )
  target_link_libraries(
    libgit2 INTERFACE
    "${LIBGIT2_LDFLAGS}"
  )
  set(LIBGIT2_LIBRARY libgit2)
  set(CMAKE_IMPORT_FILE_VERSION)
endif()

find_package_handle_standard_args(
  LIBGIT2
  DEFAULT_MSG
  LIBGIT2_PREFIX
  LIBGIT2_VERSION
  LIBGIT2_LIBRARY
)
mark_as_advanced(LIBGIT2_LIBRARY)
