/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/NfsServer.h"
#include "eden/fs/nfs/Nfsd3.h"

namespace facebook::eden {

NfsServer::NfsMountInfo NfsServer::registerMount(
    AbsolutePathPiece path,
    InodeNumber rootIno,
    std::unique_ptr<NfsDispatcher> dispatcher,
    const folly::Logger* straceLogger,
    std::shared_ptr<ProcessNameCache> processNameCache,
    folly::Duration requestTimeout,
    Notifications* FOLLY_NULLABLE notifications) {
  auto nfsd = std::make_unique<Nfsd3>(
      false,
      evb_,
      std::move(dispatcher),
      straceLogger,
      std::move(processNameCache),
      requestTimeout,
      notifications);
  mountd_.registerMount(path, rootIno);

  auto nfsdPort = nfsd->getPort();
  return {std::move(nfsd), mountd_.getPort(), nfsdPort};
}

void NfsServer::unregisterMount(AbsolutePathPiece path) {
  mountd_.unregisterMount(path);
}

} // namespace facebook::eden

#endif
