# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(generated_targets.cmake)

target_sources(
  eden_fs_notifications_windows_notifier
  INTERFACE
    ${CMAKE_CURRENT_SOURCE_DIR}/WindowsNotifier.rc
)
