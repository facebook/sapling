/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <memory>
#ifdef _WIN32
#include "eden/fs/win/mount/EdenMount.h" // @manual
#else
#include "eden/fs/inodes/EdenMount.h"
#endif
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

class StreamingSubscriber : private folly::EventBase::OnDestructionCallback {
 public:
  using Callback = std::unique_ptr<apache::thrift::StreamingHandlerCallback<
      std::unique_ptr<JournalPosition>>>;

  /** Establishes a subscription with the journal in the edenMount
   * that was passed in during construction.
   * While the subscription is active, the journal holds a reference
   * to this StreamingSubscriber and keeps it alive.
   * As part of setting this up, pushes the initial subscription information
   * to the client.
   */
  static void subscribe(
      Callback callback,
      std::shared_ptr<EdenMount> edenMount);

  // Not really public. Exposed publicly so std::make_shared can instantiate
  // this class.
  StreamingSubscriber(Callback callback, std::shared_ptr<EdenMount> edenMount);
  ~StreamingSubscriber();

 private:
  /** Schedule a call to journalUpdated.
   * The journalUpdated method will be called in the context of the
   * eventBase thread that is associated with the connected client */
  static void schedule(std::shared_ptr<StreamingSubscriber> self);

  /** Compute information to send to the connected subscriber.
   * This must only be called on the thread associated with the client.
   * This is ensured by only ever calling it via the schedule() method. */
  void journalUpdated();

  /** We implement OnDestructionCallback so that we can get notified when the
   * eventBase is about to be destroyed.  The other option for lifetime
   * management is KeepAlive tokens but those are not suitable for us
   * because we rely on the thrift eventBase threads terminating their
   * loops before we trigger our shutdown code.  KeepAlive tokens block
   * that from happening.  The next best thing is to get notified of
   * destruction and then atomically reconcile our state. */
  void onEventBaseDestruction() noexcept override;

  struct State {
    Callback callback;
    uint64_t subscriberId{0};
    bool eventBaseAlive{true};

    explicit State(Callback callback);
  };

  // There is a lock hierarchy here.  Writes to Eden update the Journal which
  // notifies the subscriber list (including StreamingSubscriber) which must
  // forward to the synchronized callback.
  // EdenMount owns and synchronizes access to the Journal, and since it's the
  // outermost entry point, its lock must always be taken before state_'s.
  //
  // It's not clear to me this is the best ordering.  It's possible to lock the
  // Journal after state_, but that would require Journal::addDelta to call
  // its callbacks outside of its lock.  Alternatively, Journal::addDelta
  // could simply schedule the subscriber calls onto the subscriber's thread.
  const std::weak_ptr<EdenMount> edenMount_;
  folly::Synchronized<State> state_;
};
} // namespace eden
} // namespace facebook
