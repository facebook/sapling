# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

IF(CMAKE_SYSTEM_NAME STREQUAL Darwin)
  rust_executable(eden_apfs_mount_helper)
  install_rust_executable(
    eden_apfs_mount_helper
    DESTINATION "bin"
  )
ENDIF()
