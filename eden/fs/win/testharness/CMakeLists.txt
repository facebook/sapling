# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB WIN_TESTHARNESS "*.cpp" "*.h")

add_library(
  eden_win_testharness
  ${WIN_TESTHARNESS}
)

target_link_libraries(
  eden_win_testharness
  PUBLIC
    eden_win
    Folly::folly
    Folly::folly_test_util
    ${LIBGMOCK_LIBRARIES}
)
