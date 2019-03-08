# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.
#
# Find libselinux
#
# This package sets:
# SELINUX_FOUND - Whether selinux was found
# SELINUX_INCLUDE_DIR - The include directory for selinux
# SELINUX_LIBRARIES - The selinux libraries
# 
include(FindPackageHandleStandardArgs)

find_path(SELINUX_INCLUDE_DIR NAMES selinux/selinux.h)
find_library(SEPOL_LIBRARY NAMES sepol)
find_library(SELINUX_LIBRARY NAMES selinux)
list(APPEND SELINUX_LIBRARIES ${SELINUX_LIBRARY} ${SEPOL_LIBRARY})
find_package_handle_standard_args(
  SELINUX
  DEFAULT_MSG
  SELINUX_INCLUDE_DIR
  SELINUX_LIBRARY
  SEPOL_LIBRARY
  SELINUX_LIBRARIES
)
mark_as_advanced(
  SELINUX_INCLUDE_DIR
  SELINUX_LIBRARY
  SEPOL_LIBRARY
  SELINUX_LIBRARIES
)
