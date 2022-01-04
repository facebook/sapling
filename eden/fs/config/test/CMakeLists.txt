# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB CONFIG_TEST_SRCS "*Test.cpp")
add_executable(
  eden_config_test
    ${CONFIG_TEST_SRCS}
)

target_link_libraries(
  eden_config_test
  PRIVATE
    eden_config
    eden_model_git
    eden_utils
    Folly::folly
    ${LIBGMOCK_LIBRARIES}
)

gtest_discover_tests(eden_config_test)
