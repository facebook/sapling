/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "RequestData.h"
#include "Dispatcher.h"

#include <glog/logging.h>

using namespace folly;
using namespace std::chrono;

namespace facebook {
namespace eden {
namespace fusell {

const std::string RequestData::kKey("fusell");

RequestData::RequestData(fuse_req_t req)
    : req_(req), requestContext_(folly::RequestContext::saveContext()) {
  fuse_req_interrupt_func(req, RequestData::interrupter, this);
}

void RequestData::interrupter(fuse_req_t req, void* data) {
  auto& request = *reinterpret_cast<RequestData*>(data);

  // Guarantee to preserve the current context
  auto saved = folly::RequestContext::saveContext();
  SCOPE_EXIT { folly::RequestContext::setContext(saved); };

  // Adopt the context of the target request
  folly::RequestContext::setContext(request.requestContext_.lock());

  if (request.interrupter_) {
    request.interrupter_->fut_.cancel();
  }
}

RequestData& RequestData::get() {
  auto data = folly::RequestContext::get()->getContextData(kKey);
  if (UNLIKELY(!data)) {
    LOG(FATAL) << "boom for missing RequestData";
    throw std::runtime_error("no fuse request data set in this context!");
  }
  return *dynamic_cast<RequestData*>(data);
}

RequestData& RequestData::create(fuse_req_t req) {
  folly::RequestContext::create();
  folly::RequestContext::get()->setContextData(
      RequestData::kKey, std::make_unique<RequestData>(req));
  return get();
}

Future<folly::Unit> RequestData::startRequest(EdenStats::Histogram& histogram) {
  startTime_ = steady_clock::now();
  DCHECK(latencyHistogram_ == nullptr);
  latencyHistogram_ = &histogram;
  return folly::Unit{};
}

void RequestData::finishRequest() {
  auto now = duration_cast<seconds>(steady_clock::now().time_since_epoch());
  auto diff = duration_cast<microseconds>(steady_clock::now() - startTime_);
  (*latencyHistogram_)->addValue(now, diff.count());
  latencyHistogram_ = nullptr;
}

fuse_req_t RequestData::stealReq() {
  fuse_req_t res = req_;
  if (res == nullptr || !req_.compare_exchange_strong(res, nullptr)) {
    throw std::runtime_error("req_ has been released");
  }
  return res;
}

fuse_req_t RequestData::getReq() const {
  if (req_ == nullptr) {
    throw std::runtime_error("req_ has been released");
  }
  return req_;
}

const fuse_ctx& RequestData::getContext() const {
  auto ctx = fuse_req_ctx(getReq());
  DCHECK(ctx != nullptr) << "request is missing its context!?";
  return *ctx;
}

Channel& RequestData::getChannel() const {
  return getDispatcher()->getChannel();
}

Dispatcher* RequestData::getDispatcher() const {
  return static_cast<Dispatcher*>(fuse_req_userdata(getReq()));
}

bool RequestData::wasInterrupted() const {
  return fuse_req_interrupted(getReq());
}

std::vector<gid_t> RequestData::getGroups() const {
  std::vector<gid_t> grps;
#if FUSE_MINOR_VERSION >= 8
  grps.resize(64);

  int ngroups = fuse_req_getgroups(getReq(), grps.size(), grps.data());
  if (ngroups < 0) {
    // OS doesn't support this operation
    return grps;
  }

  if (ngroups > grps.size()) {
    grps.resize(ngroups);
    ngroups = fuse_req_getgroups(getReq(), grps.size(), grps.data());
  }

  grps.resize(ngroups);
#endif
  return grps;
}

void RequestData::replyError(int err) {
  checkKernelError(fuse_reply_err(stealReq(), err));
}

void RequestData::replyNone() {
  fuse_reply_none(stealReq());
}

void RequestData::replyEntry(const struct fuse_entry_param& e) {
  checkKernelError(fuse_reply_entry(stealReq(), &e));
}

bool RequestData::replyCreate(const struct fuse_entry_param& e,
                              const struct fuse_file_info& fi) {
  int err = fuse_reply_create(stealReq(), &e, &fi);
  if (err == -ENOENT) {
    return false;
  } else {
    checkKernelError(err);
  }
  return true;
}

void RequestData::replyAttr(const struct stat& attr, double attr_timeout) {
  checkKernelError(fuse_reply_attr(stealReq(), &attr, attr_timeout));
}

void RequestData::replyReadLink(const std::string& link) {
  checkKernelError(fuse_reply_readlink(stealReq(), link.c_str()));
}

bool RequestData::replyOpen(const struct fuse_file_info& fi) {
  int err = fuse_reply_open(stealReq(), &fi);
  if (err == -ENOENT) {
    return false;
  } else {
    checkKernelError(err);
  }
  return true;
}

void RequestData::replyWrite(size_t count) {
  checkKernelError(fuse_reply_write(stealReq(), count));
}

void RequestData::replyBuf(const char* buf, size_t size) {
  if (size == 0) {
    buf = nullptr;
  }
  checkKernelError(fuse_reply_buf(stealReq(), buf, size));
}

void RequestData::replyIov(const struct iovec* iov, int count) {
  checkKernelError(fuse_reply_iov(stealReq(), iov, count));
}

void RequestData::replyStatfs(const struct statvfs& st) {
  checkKernelError(fuse_reply_statfs(stealReq(), &st));
}

void RequestData::replyXattr(size_t count) {
  checkKernelError(fuse_reply_xattr(stealReq(), count));
}

void RequestData::replyLock(struct flock& lock) {
  checkKernelError(fuse_reply_lock(stealReq(), &lock));
}

void RequestData::replyBmap(uint64_t idx) {
  checkKernelError(fuse_reply_bmap(stealReq(), idx));
}

void RequestData::replyIoctl(int result, const struct iovec* iov, int count) {
#ifdef FUSE_IOCTL_UNRESTRICTED
  checkKernelError(fuse_reply_ioctl(stealReq(), result, iov, count));
#else
  throwSystemErrorExplicit(ENOSYS);
#endif
}

void RequestData::replyPoll(unsigned revents) {
#if FUSE_MINOR_VERSION >= 8
  checkKernelError(fuse_reply_poll(stealReq(), revents));
#else
  throwSystemErrorExplicit(ENOSYS);
#endif
}
}
}
}
