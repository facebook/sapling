# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_HG_TEST_SRCS "*.cpp")
add_executable(
  eden_store_hg_test
  ${STORE_HG_TEST_SRCS}
)
target_link_libraries(
  eden_store_hg_test
  PUBLIC
    eden_store_hg
    eden_model
    eden_testharness
    ${LIBGMOCK_LIBRARIES}
)

if(FALSE)
  # FIXME: this needs working python_binary compat,
  # and potentially the ability to set env vars for
  # the test too.
gtest_discover_tests(eden_store_hg_test)
endif()
