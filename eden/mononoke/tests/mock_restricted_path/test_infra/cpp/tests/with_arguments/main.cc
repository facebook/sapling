// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#include <iostream>
#include <string>
#include "eden/mononoke/tests/mock_restricted_path/test_infra/cpp/tests/with_arguments/TestWithArguments.h"
#include "gtest/gtest.h"

int main(int argc, char** argv) {
  std::string command_line_arg(argc >= 2 ? argv[1] : "");
  std::cerr << "HERE: " << argv[0] << std::endl;
  testing::InitGoogleTest(&argc, argv);
  testing::AddGlobalTestEnvironment(new MyTestEnvironment(command_line_arg));
  return RUN_ALL_TESTS();
}
