/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/FieldConverter.h"

#include "eden/fs/utils/ChronoParse.h"

using folly::Expected;
using std::string;

namespace facebook::eden {

namespace {
constexpr std::array<std::string_view, 4> kEnvVars = {
    std::string_view{"HOME"},
    std::string_view{"USER"},
    std::string_view{"USER_ID"},
    std::string_view{"THRIFT_TLS_CL_CERT_PATH"},
};

/**
 * Check if string represents a well-formed file path.
 */
bool isValidAbsolutePath(string_view path) {
  // All we really care about here is making sure that
  // normalizeBestEffort() isn't going to treat the path as relatively.  We
  // probably should just add an option to normalizeBestEffort() to make it
  // reject relative paths.
  return path.starts_with(detail::kRootStr);
}
} // namespace

Expected<AbsolutePath, string> FieldConverter<AbsolutePath>::fromString(
    std::string_view value,
    const std::map<string, string>& convData) const {
  auto sString = std::string{value};
  for (auto varName : kEnvVars) {
    auto it = convData.find(std::string{varName});
    if (it != convData.end()) {
      auto envVar = fmt::format("${{{}}}", varName);
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
    return folly::makeUnexpected<string>(
        fmt::format("Cannot convert value '{}' to an absolute path", value));
  }
  // normalizeBestEffort typically will not throw, but, we want to handle
  // cases where it does, eg. getcwd fails.
  try {
    return facebook::eden::normalizeBestEffort(sString);
  } catch (const std::exception& ex) {
    return folly::makeUnexpected<string>(fmt::format(
        "Failed to convert value '{}' to an absolute path, error : {}",
        value,
        ex.what()));
  }
}

Expected<string, string> FieldConverter<string>::fromString(
    std::string_view value,
    const std::map<string, string>& /* unused */) const {
  return folly::makeExpected<string, string>(std::string{value});
}

Expected<std::chrono::nanoseconds, string>
FieldConverter<std::chrono::nanoseconds>::fromString(
    std::string_view value,
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
    std::string_view value,
    const std::map<string, string>& /* unused */) const {
  // value is a regex
  return std::make_shared<re2::RE2>(std::string{value});
}

std::string FieldConverter<std::shared_ptr<re2::RE2>>::toDebugString(
    std::shared_ptr<re2::RE2> value) const {
  if (value) {
    return value->pattern();
  }
  return "";
}

} // namespace facebook::eden
