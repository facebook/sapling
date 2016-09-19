include "common/fb303/if/fb303.thrift"

namespace cpp2 facebook.eden
namespace java com.facebook.eden.thrift
namespace py facebook.eden

/** Thrift doesn't really do unsigned numbers, but we can sort of fake it.
 * This type is serialized as an integer value that is 64-bits wide and
 * should round-trip with full fidelity for C++ client/server, but for
 * other runtimes will have crazy results if the sign bit is ever set.
 * In practice it is impossible for us to have files that large in eden,
 * and sequence numbers will take an incredibly long time to ever roll
 * over and cause problems.
 * Once t13345978 is done, we can uncomment the cpp.type below.
 */
typedef i64 /* (cpp.type = "std::uint64_t") */ unsigned64

exception EdenError {
  1: required string message
  2: optional i32 errorCode
} (message = 'message')


struct MountInfo {
  1: string mountPoint
  2: string edenClientPath
}

union SHA1Result {
  1: binary sha1
  2: EdenError error
}

// Effectively a `struct timespec`
struct TimeSpec {
  1: i64 seconds
  2: i64 nanoSeconds
}

// Information that we return when querying entries
struct FileInformation {
  1: unsigned64 size        // wish thrift had unsigned numbers
  2: TimeSpec mtime
  3: i32 mode        // mode_t
}

/** Holds information about a file, or an error in retrieving that info.
 * The most likely error will be ENOENT, implying that the file doesn't exist.
 */
union FileInformationOrError {
  1: FileInformation info
  2: EdenError error
}

/** reference a point in time in the journal.
 * This can be used to reason about a point in time in a given mount point.
 * The mountGeneration value is opaque to the client.
 */
struct JournalPosition {
  /** An opaque but unique number within the scope of a given mount point.
   * This is used to determine when sequenceNumber has been invalidated. */
  1: i64 mountGeneration

  /** Monotonically incrementing number
   * Each journalled change causes this number to increment. */
  2: unsigned64 sequenceNumber

  /** Records the snapshot hash at the appropriate point in the journal */
  3: binary snapshotHash
}

/** Holds information about a set of paths that changed between two points.
 * fromPosition, toPosition define the time window.
 * paths holds the list of paths that changed in that window.
 */
struct FileDelta {
  /** The fromPosition passed to getFilesChangedSince */
  1: JournalPosition fromPosition
  /** The current position at the time that getFilesChangedSince was called */
  2: JournalPosition toPosition
  /** The complete list of paths from both the snapshot and the overlay that
   * changed between fromPosition and toPosition */
  3: list<string> paths
}

/** Holds information about the current set of materialized files.
 * It also includes the current sequence position so that deltas
 * can be computed from this point-in-time result. */
struct MaterializedResult {
  1: JournalPosition currentPosition
  2: map<string, FileInformation> fileInfo
}

/** These map to the WM_XXX defines in the watchman wildmatch.h
 * that we'll port over to Eden real-soon-now. */
enum WildMatchFlags {
  IncludeDotFiles = 0x1,
  NoEscape = 0x2,
}

service EdenService extends fb303.FacebookService {
  list<MountInfo> listMounts() throws (1: EdenError ex)
  void mount(1: MountInfo info) throws (1: EdenError ex)
  void unmount(1: string mountPoint) throws (1: EdenError ex)

  void checkOutRevision(1: string mountPoint, 2: string hash)
    throws (1: EdenError ex)

  // Mount-specific APIs.

  /**
   * For each path, returns an EdenError instead of the SHA-1 if any of the
   * following occur:
   * - path is the empty string.
   * - path identifies a non-existent file.
   * - path identifies something that is not an ordinary file (e.g., symlink
   *   or directory).
   */
  list<SHA1Result> getSHA1(1: string mountPoint, 2: list<string> paths)

  /**
   * Returns a list of paths relative to the mountPoint.
   */
  list<string> getBindMounts(1: string mountPoint)

  /**
   * Returns the current set of files (and dirs) materialized in the overlay
   */
  MaterializedResult getMaterializedEntries(1: string mountPoint)

  /** Returns the sequence position at the time the method is called.
   * Returns the instantaneous value of the journal sequence number.
   */
  JournalPosition getCurrentJournalPosition(1: string mountPoint)

  /** Returns the set of files (and dirs) that changed since a prior point.
   * If fromPosition.mountGeneration is mismatched with the current
   * mountGeneration, throws an EdenError with errorCode = ERANGE.
   * This indicates that eden cannot compute the delta for the requested
   * range.  The client will need to recompute a new baseline using
   * other available functions in EdenService.
   */
  FileDelta getFilesChangedSince(
    1: string mountPoint,
    2: JournalPosition fromPosition)

  /** Returns a subset of the stat() information for a list of paths.
   * The returned list of information corresponds to the input list of
   * paths; eg; result[0] holds the information for paths[0].
   * We only support returning the instantaneous information about
   * these paths, as we cannot answer with historical information about
   * files in the overlay.
   */
  list<FileInformationOrError> getFileInformation(
    1: string mountPoint,
    2: list<string> paths)

  /** Returns a list of files that match the input globs.
   * There are no duplicate values in the result.
   * wildMatchFlags can hold various WildMatchFlags values OR'd together.
   */
  list<string> glob(
    1: string mountPoint,
    2: list<string> globs,
    3: i32 wildMatchFlags)
}
