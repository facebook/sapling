/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/ControlMsg.h"

#include <folly/Conv.h>
#include <stdexcept>

namespace facebook {
namespace eden {

constexpr size_t ControlMsg::kMaxFDs;

ControlMsg ControlMsg::fromMsg(
    const struct msghdr& msg,
    int level,
    int type,
    size_t expectedSize) {
  auto* cmsg = CMSG_FIRSTHDR(&msg);
  if (!cmsg) {
    throw std::runtime_error("no control data attached to msghdr");
  }
  if (cmsg->cmsg_level != level) {
    throw std::runtime_error(folly::to<std::string>(
        "unexpected control data level: ",
        static_cast<int>(cmsg->cmsg_level),
        " != ",
        level));
  }
  if (cmsg->cmsg_type != type) {
    throw std::runtime_error(folly::to<std::string>(
        "unexpected control data type: ",
        static_cast<int>(cmsg->cmsg_type),
        " != ",
        type));
  }
  if (cmsg->cmsg_len < expectedSize) {
    throw std::runtime_error(folly::to<std::string>(
        "unexpected control data length: ",
        cmsg->cmsg_len,
        " < ",
        expectedSize));
  }

  return ControlMsg(cmsg);
}

ControlMsgBuffer::ControlMsgBuffer(size_t dataLen, int level, int type)
    : capacity_{CMSG_SPACE(dataLen)}, buffer_{new uint8_t[capacity_]} {
  cmsg_ = reinterpret_cast<struct cmsghdr*>(buffer_.get());
  cmsg_->cmsg_len = CMSG_LEN(dataLen);
  cmsg_->cmsg_level = level;
  cmsg_->cmsg_type = type;
}

void ControlMsgBuffer::shrinkDataLength(size_t dataLen) {
  CHECK_LE(CMSG_SPACE(dataLen), capacity_);
  cmsg_->cmsg_len = CMSG_LEN(dataLen);
  // Update capacity_ as well.  This is required since we use the capacity_
  // field to set the msg_controllen field in the msghdr.  The kernel will
  // reject the sendmsg() call with EINVAL if msg_controllen is larger than
  // required for the specified cmsg_len.
  capacity_ = CMSG_SPACE(dataLen);
}

void ControlMsgBuffer::addToMsg(struct msghdr* msg) {
  msg->msg_control = buffer_.get();
  msg->msg_controllen = capacity_;
}
} // namespace eden
} // namespace facebook
