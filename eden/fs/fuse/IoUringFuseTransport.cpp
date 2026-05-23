/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/IoUringFuseTransport.h"

#include <stdexcept>

#include <fmt/core.h>

namespace facebook::eden {

namespace {

[[noreturn]] void throwIoUringNotImplemented(
    const char* method,
    uint32_t queueDepth) {
  throw std::runtime_error(
      fmt::format(
          "IoUringFuseTransport::{} not implemented (queueDepth={})",
          method,
          queueDepth));
}

} // namespace

IoUringFuseTransport::IoUringFuseTransport(uint32_t queueDepth)
    : queueDepth_{queueDepth} {}

const char* IoUringFuseTransport::getName() const {
  return "io_uring";
}

ssize_t IoUringFuseTransport::readInitPacket(
    int /* fd */,
    void* /* buf */,
    size_t /* size */) const {
  // Not implemented: FUSE_INIT still uses the classic /dev/fuse path.
  throwIoUringNotImplemented("readInitPacket", queueDepth_);
}

void IoUringFuseTransport::processSession(FuseChannel& /* channel */) {
  // Not implemented: io_uring request processing will be added in a follow-up
  // diff.
  throwIoUringNotImplemented("processSession", queueDepth_);
}

void IoUringFuseTransport::replyError(
    FuseChannel& /* channel */,
    const fuse_in_header& /* request */,
    int /* errorCode */) const {
  // Not implemented: io_uring reply submission will be added in a follow-up
  // diff.
  throwIoUringNotImplemented("replyError", queueDepth_);
}

void IoUringFuseTransport::sendRawReply(
    FuseChannel& /* channel */,
    const iovec[] /* iov */,
    size_t /* count */) const {
  // Not implemented: io_uring reply submission will be added in a follow-up
  // diff.
  throwIoUringNotImplemented("sendRawReply", queueDepth_);
}

} // namespace facebook::eden

#endif
