/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/common/utils/Throw.h"

#include "eden/fs/rust/redirect_ffi/include/ffi.h"

namespace facebook::eden {
Redirection redirectionFromFFI(RedirectionFFI&& redirFFI) {
  Redirection redir;
  redir.repoPath_ref() = std::string(std::move(redirFFI.repo_path));
  redir.redirType_ref() = redirectionTypeFromFFI(redirFFI.redir_type);
  redir.source_ref() = std::string(std::move(redirFFI.source));
  redir.state_ref() = redirectionStateFromFFI(redirFFI.state);
  auto optTarget = redirectionTargetFromFFI(std::move(redirFFI.target));
  if (optTarget.has_value()) {
    redir.target_ref() = std::move(optTarget.value());
  }
  return redir;
}

RedirectionType redirectionTypeFromFFI(const RedirectionTypeFFI& redirTypeFFI) {
  switch (redirTypeFFI) {
    case RedirectionTypeFFI::Bind:
      return RedirectionType::BIND;
    case RedirectionTypeFFI::Symlink:
      return RedirectionType::SYMLINK;
    case RedirectionTypeFFI::Unknown:
      return RedirectionType::UNKNOWN;
    default:
      throwf<std::runtime_error>(
          "Unknown redirection type from FFI: {}", redirTypeFFI);
  }
}

RedirectionState redirectionStateFromFFI(
    const RedirectionStateFFI& redirStateFFI) {
  switch (redirStateFFI) {
    case RedirectionStateFFI::MatchesConfiguration:
      return RedirectionState::MATCHES_CONFIGURATION;
    case RedirectionStateFFI::UnknownMount:
      return RedirectionState::UNKNOWN_MOUNT;
    case RedirectionStateFFI::NotMounted:
      return RedirectionState::NOT_MOUNTED;
    case RedirectionStateFFI::SymlinkMissing:
      return RedirectionState::SYMLINK_MISSING;
    case RedirectionStateFFI::SymlinkIncorrect:
      return RedirectionState::SYMLINK_INCORRECT;
    default:
      throwf<std::runtime_error>(
          "Unknown redirection state from FFI: {}", redirStateFFI);
  };
}

std::optional<std::string> redirectionTargetFromFFI(
    rust::String&& redirTargetFFI) {
  if (redirTargetFFI.empty()) {
    return std::nullopt;
  }
  return std::string(std::move(redirTargetFFI));
}

} // namespace facebook::eden
