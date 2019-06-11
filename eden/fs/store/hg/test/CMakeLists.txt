# Copyright (c) 2019-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

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
gtest_discover_tests(eden_store_hg_test
  EXTRA_ARGS
    --hgImportHelper
    eden/fs/store/hg/hg_import_helper.py
    --fakeHgImportHelper
    eden/fs/store/hg/test/fake_hg_import_helper.py
)
endif()
