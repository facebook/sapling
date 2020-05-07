# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB MODEL_GIT_TEST_SRCS "*Test.cpp")
add_executable(
  eden_model_git_test
    ${MODEL_GIT_TEST_SRCS}
)

target_link_libraries(
  eden_model_git_test
  PRIVATE
    eden_model_git
    ${LIBGMOCK_LIBRARIES}
)

gtest_discover_tests(eden_model_git_test)
