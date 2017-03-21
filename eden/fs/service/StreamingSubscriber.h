/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <memory>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/service/gen-cpp2/StreamingEdenService.h"

namespace facebook {
namespace eden {

/** StreamingSubscriber is used to implement pushing updates to
 * connected subscribers so that they can take action as files
 * are modified in the eden mount.
 *
 * This initial implementation is relatively dumb in that it
 * will immediately try to send a notification to the subscriber.
 *
 * Future iterations will add the ability to rate control these
 * updates (no more than 1 update per specified time interval)
 * and potentially also add a predicate so that we only notify
 * for updates that match certain criteria.
 */

class StreamingSubscriber
    : public std::enable_shared_from_this<StreamingSubscriber> {
 public:
  StreamingSubscriber(
      std::unique_ptr<apache::thrift::StreamingHandlerCallback<
          std::unique_ptr<JournalPosition>>> callback,
      std::shared_ptr<EdenMount> edenMount);
  ~StreamingSubscriber();

  /** Establishes a subscription with the journal in the edenMount
   * that was passed in during construction.
   * While the subscription is active, the journal holds a reference
   * to this StreamingSubscriber and keeps it alive.
   * As part of setting this up, pushes the initial subscription information
   * to the client.
   */
  void subscribe();

 private:
  /** Schedule a call to journalUpdated.
   * The journalUpdated method will be called in the context of the
   * eventBase thread that is associated with the connected client */
  void schedule();

  /** Compute information to send to the connected subscriber.
   * This must only be called on the thread associated with the client.
   * This is ensured by only ever calling it via the schedule() method. */
  void journalUpdated();

  std::unique_ptr<apache::thrift::StreamingHandlerCallback<
      std::unique_ptr<JournalPosition>>>
      callback_;
  std::shared_ptr<EdenMount> edenMount_;
  uint64_t subscriberId_;
};
}
}
