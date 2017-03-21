include "eden.thrift"
namespace cpp2 facebook.eden

/** This file holds definitions for the streaming flavor of the Eden interface
 * This is only available to cpp2 clients and won't compile for other
 * language/runtimes. */

service StreamingEdenService extends eden.EdenService {
  /** Request notification about changes to the journal for
   * the specified mountPoint.
   * The JournalPosition at the time of the subscribe call
   * will be pushed to the client, and then each change will
   * be pushed to the client in near-real-time.
   * The client may then use methods like getFilesChangedSince()
   * to determine the precise nature of the changes.
   */
  stream<eden.JournalPosition> subscribe(
    1: string mountPoint)
}
