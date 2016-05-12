/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "eden/fuse/fuse_headers.h"
#include <folly/io/async/Request.h>

namespace facebook {
namespace eden {
namespace fusell {

class Channel;
class Dispatcher;

class RequestData : public folly::RequestData {
  std::atomic<fuse_req_t> req_;
  // We're managed by this context, so we only keep a weak ref
  std::weak_ptr<folly::RequestContext> requestContext_;

  static void interrupter(fuse_req_t req, void* data);
  fuse_req_t stealReq();

 public:
  struct CancelBase {
    virtual ~CancelBase();
    virtual void cancel() = 0;
  };

  static const std::string kKey;
  RequestData(const RequestData&) = delete;
  RequestData& operator=(const RequestData&) = delete;
  RequestData(RequestData&&) = default;
  RequestData& operator=(RequestData&&) = default;
  explicit RequestData(fuse_req_t req);
  static RequestData& get();
  static RequestData& create(fuse_req_t req);

  // Returns the request context, which holds uid, gid, pid and umask info
  const fuse_ctx& getContext() const;

  // Returns the dispatcher embedded in the request
  Dispatcher* getDispatcher() const;

  // Returns the channel associated with the request
  Channel& getChannel() const;

  // Returns the underlying fuse request, throwing an error if it has
  // already been released
  fuse_req_t getReq() const;

  // Check whether the request has already been interrupted
  bool wasInterrupted() const;

  template <typename FUTURE>
  struct Cancel : public CancelBase {
    FUTURE fut_;
    void cancel() override { fut_.cancel(); }
    explicit Cancel(FUTURE&& fut) : fut_(std::move(fut)) {}
  };

  // Register the future chain associated with this request so that
  // we can cancel it when we receive an interrupt
  template <typename FUTURE>
  void setRequestFuture(FUTURE&& fut) {
    this->interrupter_ = std::make_unique<Cancel<FUTURE>>(std::move(fut));
  }

  // Returns the supplementary group IDs for the process making the
  // current request
  std::vector<gid_t> getGroups() const;

  // The various fuse_reply_XXX functions implicity free the request
  // pointer.  We prefer to avoid keeping a stale pointer, hence these
  // methods to maintain consistency.
  // If the replyXXX function returns false, it means that the request
  // was interrupted and that the dispatcher may need to clean up some
  // of its state.

  // Reply with a negative errno value or 0 for success
  void replyError(int err);

  // Don't send a reply, just release req_
  void replyNone();

  // Reply with a directory entry
  void replyEntry(const struct fuse_entry_param& e);

  // Reply with a directory entry and open params
  bool replyCreate(const struct fuse_entry_param& e,
                   const struct fuse_file_info& fi);

  void replyAttr(const struct stat& attr, double attr_timeout);
  void replyReadLink(const std::string& link);
  bool replyOpen(const struct fuse_file_info& fi);
  void replyWrite(size_t count);
  void replyBuf(const char* buf, size_t size);
  void replyIov(const struct iovec* iov, int count);
  void replyStatfs(const struct statvfs& st);
  void replyXattr(size_t count);
  void replyLock(struct flock& lock);
  void replyBmap(uint64_t idx);
  void replyIoctl(int result, const struct iovec* iov, int count);
  void replyPoll(unsigned revents);

 private:
  std::unique_ptr<CancelBase> interrupter_;
};
}
}
}
