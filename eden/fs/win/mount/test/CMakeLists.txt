# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB WIN_MOUNT_TEST_SRCS "*Test.cpp")

add_executable(
  eden_win_mount_test
  ${WIN_MOUNT_TEST_SRCS}
)

target_link_libraries(
  eden_win_mount_test
  PRIVATE
    eden_testharness
    eden_win_testharness
)

gtest_discover_tests(eden_win_mount_test)
