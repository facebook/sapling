/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/service/EdenError.h"

namespace facebook {
namespace eden {

EdenError newEdenError(const std::system_error& ex) {
  return newEdenError(ex.code().value(), ex.what());
}

EdenError newEdenError(const std::exception& ex) {
  const auto* edenError = dynamic_cast<const EdenError*>(&ex);
  if (edenError) {
    return *edenError;
  }
  const auto* systemError = dynamic_cast<const std::system_error*>(&ex);
  if (systemError) {
    return newEdenError(*systemError);
  }
  return EdenError(folly::exceptionStr(ex).toStdString());
}

EdenError newEdenError(const folly::exception_wrapper& ew) {
  EdenError err;
  if (!ew.with_exception([&err](const EdenError& ex) { err = ex; }) &&
      !ew.with_exception(
          [&err](const std::system_error& ex) { err = newEdenError(ex); })) {
    err = EdenError(folly::exceptionStr(ew).toStdString());
  }
  return err;
}
} // namespace eden
} // namespace facebook
