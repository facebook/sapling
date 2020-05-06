# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_executable(
  eden_inodes_test
    CheckoutTest.cpp
    DiffTest.cpp
    GlobNodeTest.cpp
    InodeBaseTest.cpp
    InodeLoaderTest.cpp
    InodeMapTest.cpp
    InodePtrTest.cpp
    InodeTimestampsTest.cpp
    RemoveTest.cpp
    TreeInodeTest.cpp
)

target_link_libraries(
  eden_inodes_test
  PRIVATE
    eden_inodes
    eden_testharness
    eden_utils
    eden_service_thrift_cpp
    Folly::folly
    ${LIBGMOCK_LIBRARIES}
)

if (WIN32)
  target_link_libraries(
    eden_inodes_test
    PRIVATE
      eden_win_utils
  )
endif()

gtest_discover_tests(eden_inodes_test)
