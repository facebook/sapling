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
#include <cstdint>
#include <memory>

namespace facebook {
namespace eden {

class JournalDelta;

/** The Journal exists to answer questions about how files are changing
 * over time.
 *
 * It contains metadata only; it is not a full snapshot of the state of
 * the filesystem at a particular point in time.
 * The intent is to be able query things like "which set of files changed
 * between time A and time B?".
 *
 * In the initial implementation we are recording file names from the overlay
 * but will expand this to record things like checking out different
 * revisions (the prior and new revision hash) from which we can derive
 * the larger list of files.
 *
 * The Journal class is not internally threadsafe; we make it safe
 * through the use of folly::Synchronized in the EdenMount class
 * that owns the Journal.
 */
class Journal {
 public:
  using SequenceNumber = uint64_t;

  /** Add a delta to the journal
   * The delta will have a new sequence number and timestamp
   * applied. */
  void addDelta(std::unique_ptr<JournalDelta>&& delta);

  /** Get a shared, immutable reference to the tip of the journal.
   * May return nullptr if there have been no changes */
  std::shared_ptr<const JournalDelta> getLatest() const;

  /** Replace the journal with a new delta.
   * The new delta will typically be the result of JournalDelta::merge().
   * No sanity checking is performed inside this function; the
   * supplied delta is moved in and replaces current tip. */
  void replaceJournal(std::unique_ptr<JournalDelta>&& delta);

 private:
  /** The sequence number that we'll use for the next entry
   * that we link into the chain */
  SequenceNumber nextSequence_{1};
  /** The most recently recorded entry */
  std::shared_ptr<const JournalDelta> latest_;
};
}
}
