# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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
