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
#include <folly/Synchronized.h>
#include <folly/stats/TimeseriesHistogram.h>

namespace facebook {
namespace eden {
namespace fusell {

class EdenStats {
 public:
  using Histogram =
      folly::Synchronized<folly::TimeseriesHistogram<int64_t>, std::mutex>;

  explicit EdenStats();

  Histogram lookup{createHistogram()};
  Histogram forget{createHistogram()};
  Histogram getattr{createHistogram()};
  Histogram setattr{createHistogram()};
  Histogram readlink{createHistogram()};
  Histogram mknod{createHistogram()};
  Histogram mkdir{createHistogram()};
  Histogram unlink{createHistogram()};
  Histogram rmdir{createHistogram()};
  Histogram symlink{createHistogram()};
  Histogram rename{createHistogram()};
  Histogram link{createHistogram()};
  Histogram open{createHistogram()};
  Histogram read{createHistogram()};
  Histogram write{createHistogram()};
  Histogram flush{createHistogram()};
  Histogram release{createHistogram()};
  Histogram fsync{createHistogram()};
  Histogram opendir{createHistogram()};
  Histogram readdir{createHistogram()};
  Histogram releasedir{createHistogram()};
  Histogram fsyncdir{createHistogram()};
  Histogram statfs{createHistogram()};
  Histogram setxattr{createHistogram()};
  Histogram getxattr{createHistogram()};
  Histogram listxattr{createHistogram()};
  Histogram removexattr{createHistogram()};
  Histogram access{createHistogram()};
  Histogram create{createHistogram()};
  Histogram getlk{createHistogram()};
  Histogram setlk{createHistogram()};
  Histogram bmap{createHistogram()};
  Histogram ioctl{createHistogram()};
  Histogram poll{createHistogram()};
  Histogram forgetmulti{createHistogram()};

 private:
  static folly::TimeseriesHistogram<int64_t> createHistogram();
};
}
}
}
