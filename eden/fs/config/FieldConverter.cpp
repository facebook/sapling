/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/config/FieldConverter.h"

using folly::Expected;
using std::string;

namespace {
constexpr std::array<folly::StringPiece, 3> kEnvVars = {
    folly::StringPiece{"HOME"},
    folly::StringPiece{"USER"},
    folly::StringPiece{"USER_ID"},
};

/**
 * Check if string represents a well-formed file path.
 */
bool isValidAbsolutePath(folly::StringPiece path) {
  // TODO: we should probably move this into PathFuncs.cpp and consolidate it
  // with some of the logic in AbsolutePathSanityCheck.
  //
  // Alternatively, all we really care about here is making sure that
  // normalizeBestEffort() isn't going to treat the path as relatively.  We
  // probably should just add an option to normalizeBestEffort() to make it
  // reject relative paths.
  return path.startsWith(facebook::eden::kDirSeparator);
}
} // namespace

namespace facebook {
namespace eden {

Expected<AbsolutePath, string> FieldConverter<AbsolutePath>::fromString(
    folly::StringPiece value,
    const std::map<string, string>& convData) const {
  auto sString = value.str();
  for (auto varName : kEnvVars) {
    auto it = convData.find(varName.str());
    if (it != convData.end()) {
      auto envVar = folly::to<string>("${", varName, "}");
      // There may be multiple ${USER} tokens to replace, so loop
      // until we've processed all of them
      while (true) {
        auto idx = sString.find(envVar);
        if (idx == string::npos) {
          break;
        }
        sString.replace(idx, envVar.size(), it->second);
      }
    }
  }

  if (!::isValidAbsolutePath(sString)) {
    return folly::makeUnexpected<string>(folly::to<string>(
        "Cannot convert value '", value, "' to an absolute path"));
  }
  // normalizeBestEffort typically will not throw, but, we want to handle
  // cases where it does, eg. getcwd fails.
  try {
    return facebook::eden::normalizeBestEffort(sString);
  } catch (const std::exception& ex) {
    return folly::makeUnexpected<string>(folly::to<string>(
        "Failed to convert value '",
        value,
        "' to an absolute path, error : ",
        ex.what()));
  }
}

Expected<string, string> FieldConverter<string>::fromString(
    folly::StringPiece value,
    const std::map<string, string>& /* unused */) const {
  return folly::makeExpected<string, string>(value.toString());
}

} // namespace eden
} // namespace facebook
