/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/prjfs/PrjfsRequestContext.h"

namespace facebook::eden {
void PrjfsRequestContext::handleException(folly::Try<folly::Unit> try_) const {
  XDCHECK(try_.hasException());

  auto* exc = try_.tryGetExceptionObject<std::exception>();
  sendError(exceptionToHResult(*exc));

  XLOG(DBG5) << folly::exceptionStr(*exc);
}
} // namespace facebook::eden

#endif
