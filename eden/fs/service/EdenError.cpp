/*
 *  Copyright (c) 2017, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenError.h"

namespace facebook {
namespace eden {

EdenError newEdenError(int errorCode, folly::StringPiece message) {
  auto e = EdenError(message.str());
  e.set_errorCode(errorCode);
  return e;
}

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
}
}
