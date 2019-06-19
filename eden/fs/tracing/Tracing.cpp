/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "Tracing.h"

namespace facebook {
namespace eden {
namespace detail {
Tracer globalTracer;

void ThreadLocalTracePoints::flush() {
  auto points = globalTracer.tracepoints_.wlock();
  auto state = state_.lock();
  size_t npoints = std::min(kBufferPoints, state->currNum_);
  points->insert(
      points->end(),
      state->tracePoints_.begin(),
      state->tracePoints_.begin() + npoints);
  state->currNum_ = 0;
}

folly::RequestToken tracingToken("eden_tracing");

std::vector<CompactTracePoint> Tracer::getAllTracepoints() {
  for (auto& tltp : tltp_.accessAllThreads()) {
    tltp.flush();
  }
  auto points = tracepoints_.wlock();
  std::sort(points->begin(), points->end(), [](const auto& a, const auto& b) {
    return a.timestamp < b.timestamp;
  });
  return std::move(*points);
}
} // namespace detail
} // namespace eden
} // namespace facebook
