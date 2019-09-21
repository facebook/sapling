# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(FindPackageHandleStandardArgs)
find_package(PkgConfig)

pkg_check_modules(LibGit2 libgit2 QUIET IMPORTED_TARGET)
if(LibGit2_FOUND)
  set_target_properties(PkgConfig::LibGit2 PROPERTIES IMPORTED_GLOBAL True)
  add_library(libgit2 ALIAS PkgConfig::LibGit2)
endif()

find_package_handle_standard_args(
  LibGit2
  REQUIRED_VARS LibGit2_PREFIX
  VERSION_VAR LibGit2_VERSION
)
