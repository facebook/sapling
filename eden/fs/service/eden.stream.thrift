include "eden.thrift"
namespace cpp2 facebook.eden

/** This file holds definitions for the streaming flavor of the Eden interface
 * This is only available to cpp2 clients and won't compile for other
 * language/runtimes. */

service StreamingEdenService extends EdenService {
  /** Prototype of the subscription stream for watchman.
   * We'll certainly want to add some filter predicates to this, but I don't
   * want to get hung up on this in this draft of the API.  The caller can
   * subscribe to the stream of file changes and do something with the data as
   * it streams in.
   * The intended usage pattern is for the client to call
   * getCurrentJournalPosition() to get an initial basis, and then pass that
   * value in to subscribe to receive deltas from that point in time.  This is
   * pretty coarse right now because it will propagate all changes through to
   * watchman.  The initial integration can do this and allow watchman to
   * filter locally.  In the longer run we'll want to add a predicate to this
   * so that we reduce the bandwidth needed between eden and watchman.
   */
  stream<FileDelta> subscribe(
    1: string mountPoint,
    2: JournalPosition fromPosition)
}
