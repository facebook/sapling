/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <folly/FBVector.h>
#include <folly/io/IOBuf.h>

namespace facebook {
namespace eden {

/**
 * Represents data that may come from a buffer or a file descriptor.
 *
 * While we don't currently have a fuse client lib that supports this,
 * we want to make sure we're ready to use it, so this looks like
 * a dumb wrapper around IOBuf at the moment.
 */
class BufVec {
  struct Buf {
    std::unique_ptr<folly::IOBuf> buf;
    int fd{-1};
    size_t fd_size{0};
    off_t fd_pos{-1};

    Buf(const Buf&) = delete;
    Buf& operator=(const Buf&) = delete;
    Buf(Buf&&) = default;
    Buf& operator=(Buf&&) = default;

    explicit Buf(std::unique_ptr<folly::IOBuf> buf);
  };
  folly::fbvector<std::shared_ptr<Buf>> items_;

 public:
  BufVec(const BufVec&) = delete;
  BufVec& operator=(const BufVec&) = delete;
  BufVec(BufVec&&) = default;
  BufVec& operator=(BufVec&&) = default;

  explicit BufVec(std::unique_ptr<folly::IOBuf> buf);

  /**
   * Return an iovector suitable for e.g. writev()
   *   auto iov = buf->getIov();
   *   auto xfer = writev(fd, iov.data(), iov.size());
   */
  folly::fbvector<struct iovec> getIov() const;

  /**
   * Returns the total number of bytes in the BufVec.
   */
  size_t size() const;

  /**
   * Copies the buffer into a std::string.
   */
  std::string copyData() const;
};

} // namespace eden
} // namespace facebook
