# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_executable(
  eden_inodes_test
    CheckoutTest.cpp
    DiffTest.cpp
    GlobNodeTest.cpp
    InodeBaseTest.cpp
    VirtualInodeLoaderTest.cpp
    VirtualInodeTest.cpp
    InodeMapTest.cpp
    InodePtrTest.cpp
    InodeTimestampsTest.cpp
    RemoveTest.cpp
    RenameTest.cpp
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

gtest_discover_tests(eden_inodes_test)
