/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/FieldConverter.h"

#include <folly/Conv.h>

#include "eden/fs/utils/ChronoParse.h"

using folly::Expected;
using std::string;

namespace facebook::eden {

namespace {
constexpr std::array<folly::StringPiece, 4> kEnvVars = {
    folly::StringPiece{"HOME"},
    folly::StringPiece{"USER"},
    folly::StringPiece{"USER_ID"},
    folly::StringPiece{"THRIFT_TLS_CL_CERT_PATH"},
};

/**
 * Check if string represents a well-formed file path.
 */
bool isValidAbsolutePath(folly::StringPiece path) {
  // All we really care about here is making sure that
  // normalizeBestEffort() isn't going to treat the path as relatively.  We
  // probably should just add an option to normalizeBestEffort() to make it
  // reject relative paths.
  return path.startsWith(detail::kRootStr);
}
} // namespace

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

  if (!isValidAbsolutePath(sString)) {
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

Expected<std::chrono::nanoseconds, string>
FieldConverter<std::chrono::nanoseconds>::fromString(
    folly::StringPiece value,
    const std::map<string, string>& /* unused */) const {
  auto result = stringToDuration(value);
  if (result.hasValue()) {
    return result.value();
  }
  return folly::makeUnexpected(chronoParseErrorToString(result.error()).str());
}

std::string FieldConverter<std::chrono::nanoseconds>::toDebugString(
    std::chrono::nanoseconds value) const {
  return durationToString(value);
}

Expected<std::shared_ptr<re2::RE2>, string>
FieldConverter<std::shared_ptr<re2::RE2>>::fromString(
    folly::StringPiece value,
    const std::map<string, string>& /* unused */) const {
  // value is a regex
  return std::make_shared<re2::RE2>(value.str());
}

std::string FieldConverter<std::shared_ptr<re2::RE2>>::toDebugString(
    std::shared_ptr<re2::RE2> value) const {
  if (value) {
    return value->pattern();
  }
  return "";
}

} // namespace facebook::eden
