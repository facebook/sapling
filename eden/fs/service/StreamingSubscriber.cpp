/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "StreamingSubscriber.h"

#include <folly/experimental/logging/xlog.h>

using folly::StringPiece;

namespace facebook {
namespace eden {

StreamingSubscriber::State::State(
    StreamingSubscriber::Callback callback,
    std::weak_ptr<EdenMount> edenMount)
    : callback(std::move(callback)), edenMount(edenMount) {}

StreamingSubscriber::StreamingSubscriber(
    Callback callback,
    std::shared_ptr<EdenMount> edenMount)
    : state_(folly::in_place, std::move(callback), std::move(edenMount)) {
  auto state = state_.wlock();
  // Arrange to be told when the eventBase is about to be destroyed
  state->callback->getEventBase()->runOnDestruction(this);
}

void StreamingSubscriber::runLoopCallback() noexcept {
  auto state = state_.wlock();
  if (state->callback) {
    // We're called on the eventBase thread so we can call these
    // methods directly and tear down the peer.  Note that we
    // should only get here in the case that the server is being
    // shutdown.  The individual unmount case is handled by the
    // destructor.
    state->callback->done();
    state->callback.reset();
  }
  state->eventBaseAlive = false;
}

StreamingSubscriber::~StreamingSubscriber() {
  auto state = state_.wlock();
  // If the eventBase is still live then we should tear down the peer
  if (state->callback && state->eventBaseAlive) {
    auto evb = state->callback->getEventBase();

    // Move the callback away; we won't be able to use it
    // via state-> again.
    evb->runInEventBaseThread(
        [callback = std::move(state->callback)]() mutable {
          callback->done();
          callback.reset();
        });
  }
}

void StreamingSubscriber::subscribe() {
  // Separately scope the wlock as the schedule() below will attempt
  // to acquire the lock for itself.
  {
    auto state = state_.wlock();

    auto edenMount = state->edenMount.lock();
    DCHECK(edenMount)
        << "we're called with the owner referenced, so this should always be valid";
    state->subscriberId = edenMount->getJournal().wlock()->registerSubscriber(
        [self = shared_from_this()]() { self->schedule(); });
  }

  // Suggest to the subscription that the journal has been updated so that
  // it will compute initial delta information.
  schedule();
}

void StreamingSubscriber::schedule() {
  auto state = state_.rlock();
  if (state->callback) {
    state->callback->getEventBase()->runInEventBaseThread(
        [self = shared_from_this()]() { self->journalUpdated(); });
  }
}

void StreamingSubscriber::journalUpdated() {
  auto state = state_.wlock();

  if (!state->callback) {
    // We were cancelled while this callback was queued up.
    // There's nothing for us to do now.
    return;
  }

  auto edenMount = state->edenMount.lock();
  bool tearDown = !edenMount || !state->callback->isRequestActive();

  if (!tearDown &&
      !edenMount->getJournal().rlock()->isSubscriberValid(
          state->subscriberId)) {
    tearDown = true;
  }

  if (tearDown) {
    XLOG(DBG1) << "Subscription is no longer active";
    if (edenMount) {
      edenMount->getJournal().wlock()->cancelSubscriber(state->subscriberId);
    }
    state->callback->done();
    state->callback.reset();
    return;
  }

  JournalPosition pos;

  auto delta = edenMount->getJournal().rlock()->getLatest();
  pos.sequenceNumber = delta->toSequence;
  pos.snapshotHash = StringPiece(delta->toHash.getBytes()).str();
  pos.mountGeneration = edenMount->getMountGeneration();

  try {
    // And send it
    state->callback->write(pos);
  } catch (const std::exception& exc) {
    XLOG(ERR) << "Error while sending subscription update: " << exc.what();
  }
}
}
}
