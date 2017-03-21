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

using folly::StringPiece;

namespace facebook {
namespace eden {

StreamingSubscriber::StreamingSubscriber(
    std::unique_ptr<apache::thrift::StreamingHandlerCallback<
        std::unique_ptr<JournalPosition>>> callback,
    std::shared_ptr<EdenMount> edenMount)
    : callback_(std::move(callback)), edenMount_(std::move(edenMount)) {}

StreamingSubscriber::~StreamingSubscriber() {
  // NOTE: we can't call callback_->done() directly from here as there is no
  // guarantee that we'd be destroyed on the correct thread!
}

void StreamingSubscriber::subscribe() {
  subscriberId_ = edenMount_->getJournal()
                      .wlock()
                      ->registerSubscriber([self = shared_from_this()]() {
                        self->schedule();
                      });

  // Suggest to the subscription that the journal has been updated so that
  // it will compute initial delta information.
  schedule();
}

void StreamingSubscriber::schedule() {
  callback_->getEventBase()
      ->runInEventBaseThread([self = shared_from_this()]() {
        self->journalUpdated();
      });
}

void StreamingSubscriber::journalUpdated() {
  if (!callback_) {
    // We were cancelled while this callback was queued up.
    // There's nothing for us to do now.
    return;
  }

  if (!callback_->isRequestActive()) {
    // Peer disconnected, so tear down the subscription
    // TODO: is this the right way to detect this?
    VLOG(1) << "Subscription is no longer active";
    edenMount_->getJournal().wlock()->cancelSubscriber(subscriberId_);
    callback_->done();
    callback_.reset();
    return;
  }

  JournalPosition pos;

  auto delta = edenMount_->getJournal().rlock()->getLatest();
  pos.sequenceNumber = delta->toSequence;
  pos.snapshotHash = StringPiece(delta->toHash.getBytes()).str();
  pos.mountGeneration = edenMount_->getMountGeneration();

  try {
    // And send it
    callback_->write(pos);
  } catch (const std::exception& exc) {
    LOG(ERROR) << "Error while sending subscription update: " << exc.what();
  }
}
}
}
