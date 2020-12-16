/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/prjfs/PrjfsRequestContext.h"

namespace facebook::eden {
void PrjfsRequestContext::handleException(
    folly::Try<folly::Unit> try_,
    Notifications* FOLLY_NULLABLE notifications) const {
  XDCHECK(try_.hasException());

  auto* exc = try_.tryGetExceptionObject<std::exception>();
  sendError(exceptionToHResult(*exc));

  if (auto* err = dynamic_cast<const folly::FutureTimeout*>(exc)) {
    XLOG_EVERY_MS(WARN, 1000)
        << "Prjfs request timed out: " << folly::exceptionStr(*err);
    if (notifications) {
      notifications->showGenericErrorNotification(*err);
    }
  } else {
    XLOG(DBG5) << folly::exceptionStr(*exc);
  }
}
} // namespace facebook::eden

#endif
