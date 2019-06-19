# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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
  target_link_options(
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
