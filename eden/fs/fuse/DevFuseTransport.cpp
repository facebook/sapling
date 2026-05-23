/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/DevFuseTransport.h"

#include "eden/fs/fuse/FuseChannel.h"

#include <unistd.h>

namespace facebook::eden {

const char* DevFuseTransport::getName() const {
  return "devfuse";
}

size_t DevFuseTransport::getWorkerThreadCount(size_t defaultThreadCount) const {
  return defaultThreadCount;
}

ssize_t DevFuseTransport::readInitPacket(int fd, void* buf, size_t size) const {
  return read(fd, buf, size);
}

void DevFuseTransport::processSession(FuseChannel& channel) {
  channel.processDevFuseSession();
}

void DevFuseTransport::replyError(
    FuseChannel& channel,
    const fuse_in_header& request,
    int errorCode) const {
  channel.replyErrorDevFuse(request, errorCode);
}

void DevFuseTransport::sendRawReply(
    FuseChannel& channel,
    const iovec iov[],
    size_t count) const {
  channel.sendRawReplyDevFuse(iov, count);
}

} // namespace facebook::eden

#endif
