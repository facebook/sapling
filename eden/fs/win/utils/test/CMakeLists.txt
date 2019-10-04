# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB WIN_UTILS_TEST_SRCS "*Test.cpp")

# This is trying to link the executable will both static and dynamic version of
# Boost, which fails the build. Taking out FileUtilsTest.cpp until we figure
# that out.

list(
    REMOVE_ITEM WIN_UTILS_TEST_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/FileUtilsTest.cpp
  )

add_executable(
  eden_win_utils_test
  ${WIN_UTILS_TEST_SRCS}
)

target_link_libraries(
  eden_win_utils_test
  PRIVATE
    eden_win_utils
    ${LIBGMOCK_LIBRARIES}
)

gtest_discover_tests(eden_win_utils_test)
