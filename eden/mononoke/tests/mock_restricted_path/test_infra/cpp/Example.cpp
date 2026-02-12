// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#include "eden/mononoke/tests/mock_restricted_path/test_infra/cpp/Example.h"

namespace facebook::example {

std::string Example::indexToString(size_t index) {
  switch (index) {
    case 0:
      return "zero";
    case 1:
      return "one";
    case 2:
      return "two";
    default:
      return "got bored and will stop here";
  }
}

} // namespace facebook::example
