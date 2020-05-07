# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB MODEL_TEST_SRCS "*Test.cpp")
add_executable(
  eden_model_test
    ${MODEL_TEST_SRCS}
)

target_link_libraries(
  eden_model_test
  PRIVATE
    eden_model
    eden_testharness
    ${LIBGMOCK_LIBRARIES}
)

if (WIN32)
  target_link_libraries(
    eden_model_test
    PRIVATE
      eden_win_utils
  )
endif()

gtest_discover_tests(eden_model_test)
