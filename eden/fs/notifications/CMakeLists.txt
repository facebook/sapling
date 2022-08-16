# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB NOTIFICATIONS_SRCS "*.cpp")
add_library(
  eden_notifications STATIC
  ${NOTIFICATIONS_SRCS}
)
target_link_libraries(
  eden_notifications
  PUBLIC
    eden_config
    edencommon::edencommon_utils
)
target_sources(
  eden_notifications
  INTERFACE
    ${CMAKE_CURRENT_SOURCE_DIR}/WindowsNotifier.rc
)
