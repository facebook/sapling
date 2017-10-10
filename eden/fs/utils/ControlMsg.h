/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <glog/logging.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <memory>

namespace facebook {
namespace eden {

/**
 * Helper class for accessing a socket cmsghdr.
 *
 * This wraps a cmsghdr pointer and provides utility functions for working with
 * it.  It does not contain data storage for the cmsghdr.  Use ControlMsgBuffer
 * if you also need data storage for the cmsghdr.
 *
 * This class is suitable for processing cmsghdr information received with
 * recvmsg().
 */
class ControlMsg {
 public:
  /**
   * The maximum number of file descriptors that can be sent in a SCM_RIGHTS
   * control message.
   *
   * Linux internally defines this to 253 using the SCM_MAX_FD constant in
   * linux/include/net/scm.h
   */
  static constexpr size_t kMaxFDs = 253;

  /**
   * Create a ControlMsg object from a msghdr received with recvmsg()
   *
   * This checks that cmsg data was attached to the received message,
   * and is of the expected level, type, and length.
   */
  static ControlMsg
  fromMsg(const struct msghdr& msg, int level, int type, size_t expectedSize);

  /**
   * Get a pointer to the cmsghdr struct.
   */
  struct cmsghdr* getCmsg() {
    return cmsg_;
  }
  const struct cmsghdr* getCmsg() const {
    return cmsg_;
  }

  /**
   * Get the cmsg data length.
   */
  size_t getDataLength() const {
    return cmsg_->cmsg_len - CMSG_LEN(0);
  }

  /**
   * Access the cmsg data as a pointer to the desired data type.
   */
  template <typename T>
  T* getData() {
    DCHECK_LE(sizeof(T), getDataLength());
    return reinterpret_cast<T*>(CMSG_DATA(getCmsg()));
  }

 protected:
  ControlMsg() {}
  explicit ControlMsg(struct cmsghdr* cmsg) : cmsg_{cmsg} {}

  struct cmsghdr* cmsg_{nullptr};
};

/**
 * ControlMsgBuffer extends ControlMsg and also provides a buffer to store
 * cmsghdr data.
 *
 * This class is suitable for building cmsghdr objects to send with sendmsg().
 */
class ControlMsgBuffer : public ControlMsg {
 public:
  /**
   * Create a cmsghdr with the specified data length, level, and type.
   */
  explicit ControlMsgBuffer(size_t dataLen, int level, int type);

  size_t getCapacity() const {
    return capacity_;
  }

  /**
   * Shrink the data length in the cmsg structure.
   *
   * This can be used to shrink the data length if you need less than was
   * originally allocated.
   */
  void shrinkDataLength(size_t dataLen);

  /**
   * Attach this control message to a msghdr object, so it can be sent with
   * sendmsg().
   */
  void addToMsg(struct msghdr* msg);

 private:
  size_t capacity_{0};
  std::unique_ptr<uint8_t[]> buffer_;
};
} // namespace eden
} // namespace facebook
