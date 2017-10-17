include "common/fb303/if/fb303.thrift"
include "eden/fs/inodes/hgdirstate.thrift"

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

/**
 * A source control hash, as a 20-byte binary value.
 */
typedef binary BinaryHash

exception EdenError {
  1: required string message
  2: optional i32 errorCode
} (message = 'message')

exception NoValueForKeyError {
  1: string key
}

struct MountInfo {
  1: string mountPoint
  2: string edenClientPath
}

union SHA1Result {
  1: BinaryHash sha1
  2: EdenError error
}

/**
 * Effectively a `struct timespec`
 */
struct TimeSpec {
  1: i64 seconds
  2: i64 nanoSeconds
}

/**
 * Information that we return when querying entries
 */
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
  3: BinaryHash snapshotHash
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
  3: list<string> changedPaths
  4: list<string> createdPaths
  5: list<string> removedPaths
  /** When fromPosition.snapshotHash != toPosition.snapshotHash this holds
   * the union of the set of files whose StatusCode differed from the
   * committed fromPosition hash before the hash changed, and the set of
   * files whose StatusCode differed from the committed toPosition hash
   * after the hash was changed.  This list of files represents files
   * whose state may have changed as part of an update operation, but
   * in ways that may not be able to be extracted solely by performing
   * source control diff operations on the from/to hashes. */
  6: list<string> uncleanPaths
}

enum StatusCode {
  CLEAN = 0x0,
  MODIFIED = 0x1,
  ADDED = 0x2,
  REMOVED = 0x3,
  MISSING = 0x4,
  NOT_TRACKED = 0x5,
  IGNORED = 0x6,
}

struct ThriftHgStatus {
  1: map<string, StatusCode> entries
}

enum ConflictType {
  /**
   * We failed to update this particular path due to an error
   */
  ERROR = 0,
  /**
   * A locally modified file was deleted in the new Tree
   */
  MODIFIED_REMOVED = 1,
  /**
   * An untracked local file exists in the new Tree
   */
  UNTRACKED_ADDED = 2,
  /**
   * The file was removed locally, but modified in the new Tree
   */
  REMOVED_MODIFIED = 3,
  /**
   * The file was removed locally, and also removed in the new Tree.
   */
  MISSING_REMOVED = 4,
  /**
   * A locally modified file was modified in the new Tree
   * This may be contents modifications, or a file type change (directory to
   * file or vice-versa), or permissions changes.
   */
  MODIFIED_MODIFIED = 5,
  /**
   * A directory was supposed to be removed or replaced with a file,
   * but it contains untracked files preventing us from updating it.
   */
  DIRECTORY_NOT_EMPTY = 6,
}

/**
 * Details about conflicts or errors that occurred during a checkout operation
 */
struct CheckoutConflict {
  1: string path
  2: ConflictType type
  3: string message
}

struct ScmBlobMetadata {
  1: i64 size
  2: BinaryHash contentsSha1
}

struct ScmTreeEntry {
  1: binary name
  2: i32 mode
  3: BinaryHash id
}

struct HgNonnormalFile {
  1: string relativePath
  2: hgdirstate.DirstateTuple tuple
}

struct TreeInodeEntryDebugInfo {
  /**
   * The entry name.  This is just a PathComponent, not the full path
   */
  1: binary name
  /**
   * The inode number, or 0 if no inode number has been assigned to
   * this entry
   */
  2: i64 inodeNumber
  /**
   * The entry mode_t value
   */
  3: i32 mode
  /**
   * True if an InodeBase object exists for this inode or not.
   */
  4: bool loaded
  /**
   * True if an the inode is materialized in the overlay
   */
  5: bool materialized
  /**
   * If materialized is false, hash contains the ID of the underlying source
   * control Blob or Tree.
   */
  6: BinaryHash hash
}

struct WorkingDirectoryParents {
  1: BinaryHash parent1
  2: optional BinaryHash parent2
}

struct TreeInodeDebugInfo {
  1: i64 inodeNumber
  2: binary path
  3: bool materialized
  4: BinaryHash treeHash
  5: list<TreeInodeEntryDebugInfo> entries
  6: i64 refcount
}

struct InodePathDebugInfo {
  1: string path
  2: bool loaded
  3: bool linked
}

/**
 * Struct to store Information about inodes in a mount point.
 */
struct MountInodeInfo {
  1: i64 loadedInodeCount
  2: i64 unloadedInodeCount
  3: i64 materializedInodeCount
}

/**
 * Struct to store fb303 counters from ServiceData.getCounters() and inode
 * information of all the mount points.
 */
struct InternalStats {
  1: i64 periodicUnloadCount
/**
 * counters is the list of fb303 counters, key is the counter name, value is the
 * counter value.
 */
  2: map<string, i64> counters
/**
 * mountPointInfo is a map whose key is the path of the mount point and value
 * is the details like number of loaded inodes,unloaded inodes in that mount
 * and number of materialized inodes in that mountpoint.
 */
  3: map<string, MountInodeInfo> mountPointInfo
}

service EdenService extends fb303.FacebookService {
  list<MountInfo> listMounts() throws (1: EdenError ex)
  void mount(1: MountInfo info) throws (1: EdenError ex)
  void unmount(1: string mountPoint) throws (1: EdenError ex)

  /**
   * Get the parent commit(s) of the working directory
   */
  WorkingDirectoryParents getParentCommits(1: string mountPoint)
    throws (1: EdenError ex)

  /**
   * Check out the specified snapshot.
   *
   * This updates the contents of the mount point so that they match the
   * contents of the given snapshot.
   *
   * Returns a list of conflicts and errors that occurred when performing the
   * checkout operation.
   *
   * If the force parameter is true, the working directory will be forcibly
   * updated to the contents of the new snapshot, even if there were conflicts.
   * Conflicts will still be reported in the return value, but the files will
   * be updated to their new state.  If the force parameter is false files with
   * conflicts will be left unmodified.  Files that are untracked in both the
   * source and destination snapshots are always left unchanged, even if force
   * is true.
   *
   * On successful return from this function the mount point will point to the
   * new commit, even if some paths had conflicts or errors.  The caller is
   * responsible for taking appropriate action to update these paths as desired
   * after checkOutRevision() returns.
   */
  list<CheckoutConflict> checkOutRevision(
    1: string mountPoint,
    2: BinaryHash snapshotHash,
    3: bool force)
      throws (1: EdenError ex)

  /**
   * Reset the working directory's parent commits, without changing the working
   * directory contents.
   *
   * This operation is equivalent to `git reset --soft` or `hg reset --keep`
   */
  void resetParentCommits(
    1: string mountPoint,
    2: WorkingDirectoryParents parents)
      throws (1: EdenError ex)

  /**
   * For each path, returns an EdenError instead of the SHA-1 if any of the
   * following occur:
   * - path is the empty string.
   * - path identifies a non-existent file.
   * - path identifies something that is not an ordinary file (e.g., symlink
   *   or directory).
   */
  list<SHA1Result> getSHA1(1: string mountPoint, 2: list<string> paths)
    throws (1: EdenError ex)

  /**
   * Returns a list of paths relative to the mountPoint.
   */
  list<string> getBindMounts(1: string mountPoint)
    throws (1: EdenError ex)

  /** Returns the sequence position at the time the method is called.
   * Returns the instantaneous value of the journal sequence number.
   */
  JournalPosition getCurrentJournalPosition(1: string mountPoint)
    throws (1: EdenError ex)

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
      throws (1: EdenError ex)

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
      throws (1: EdenError ex)

  /** Returns a list of files that match the input globs.
   * There are no duplicate values in the result.
   * wildMatchFlags can hold various WildMatchFlags values OR'd together.
   */
  list<string> glob(
    1: string mountPoint,
    2: list<string> globs)
      throws (1: EdenError ex)

  //////// Source Control APIs ////////

  // TODO(mbolin): `hg status` has a ton of command line flags to support.
  ThriftHgStatus scmGetStatus(
    1: string mountPoint,
    2: bool listIgnored,
  ) throws (1: EdenError ex)

  void hgClearDirstate(
    1: string mountPoint,
  ) throws (1: EdenError ex)

  void hgSetDirstateTuple(
    1: string mountPoint,
    2: string relativePath,
    3: hgdirstate.DirstateTuple tuple,
  ) throws (1: EdenError ex)

  // Throw KeyError if no entry for relativePath?
  hgdirstate.DirstateTuple hgGetDirstateTuple(
    1: string mountPoint,
    2: string relativePath,
  ) throws (
    1: EdenError ex
    2: NoValueForKeyError noValueForKeyError
  )

  /** Return a boolean indicating whether something was actually deleted. */
  bool hgDeleteDirstateTuple(
    1: string mountPoint,
    2: string relativePath,
  ) throws (1: EdenError ex)

  list<HgNonnormalFile> hgGetNonnormalFiles(
    1: string mountPoint,
  ) throws (1: EdenError ex)

  // If relativePathSource is the empty string, remove the entry in the map for
  // relativePathDest.
  void hgCopyMapPut(
    1: string mountPoint,
    2: string relativePathDest,
    3: string relativePathSource,
  )

  string hgCopyMapGet(
    1: string mountPoint,
    2: string relativePathDest,
  ) throws (1: NoValueForKeyError noValueForKeyError)

  /**
   * In practice, this map should be fairly small.
   */
  map<string, string> hgCopyMapGetAll(
    1: string mountPoint,
  )

  //////// Debugging APIs ////////

  /**
   * Get the contents of a source control Tree.
   *
   * This can be used to confirm if eden's LocalStore contains information
   * for the tree, and that the information is correct.
   *
   * If localStoreOnly is true, the data is loaded directly from the
   * LocalStore, and an error will be raised if it is not already present in
   * the LocalStore.  If localStoreOnly is false, the data may be retrieved
   * from the BackingStore if it is not already present in the LocalStore.
   */
  list<ScmTreeEntry> debugGetScmTree(
    1: string mountPoint,
    2: BinaryHash id,
    3: bool localStoreOnly,
  ) throws (1: EdenError ex)

  /**
   * Get the contents of a source control Blob.
   *
   * This can be used to confirm if eden's LocalStore contains information
   * for the blob, and that the information is correct.
   */
  binary debugGetScmBlob(
    1: string mountPoint,
    2: BinaryHash id,
    3: bool localStoreOnly,
  ) throws (1: EdenError ex)

  /**
   * Get the metadata about a source control Blob.
   *
   * This retrieves the metadata about a source control Blob.  This returns
   * the size and contents SHA1 of the blob, which eden stores separately from
   * the blob itself.  This can also be a useful alternative to
   * debugGetScmBlob() when getting data about extremely large blobs.
   */
  ScmBlobMetadata debugGetScmBlobMetadata(
    1: string mountPoint,
    2: BinaryHash id,
    3: bool localStoreOnly,
  ) throws (1: EdenError ex)

  /**
   * Get status about currently loaded inode objects.
   *
   * This returns details about all currently loaded inode objects under the
   * given path.
   *
   * If the path argument is the empty string data will be returned about all
   * inodes in the entire mount point.  Otherwise the path argument should
   * refer to a subdirectory, and data will be returned for all inodes under
   * the specified subdirectory.
   *
   * The rename lock is not held while gathering this information, so the path
   * name information returned may not always be internally consistent.  If
   * renames were taking place while gathering the data, some inodes may show
   * up under multiple parents.  It's also possible that we may miss some
   * inodes during the tree walk if they were renamed from a directory that was
   * not yet walked into a directory that has already been walked.
   *
   * This API cannot return data about inodes that have been unlinked but still
   * have outstanding references.
   */
  list<TreeInodeDebugInfo> debugInodeStatus(
    1: string mountPoint,
    2: string path,
  ) throws (1: EdenError ex)

  /**
   * Get the InodePathDebugInfo for the inode that corresponds to the given
   * inode number. This provides the path for the inode and also indicates
   * whether the inode is currently loaded or not. Requires that the Eden
   * mountPoint be specified.
   */
  InodePathDebugInfo debugGetInodePath(
    1: string mountPoint,
    2: i64 inodeNumber,
  ) throws (1: EdenError ex)

  /**
   * Sets the log level for a given category at runtime.
   */
  void debugSetLogLevel(
    1: string category,
    2: string level,
  ) throws (1: EdenError ex)

  /**
  * Unloads unused Inodes from a directory inside a mountPoint whose last
  * access time is older than the specified age.
  *
  * The age parameter is a relative time to be subtracted from the current
  * (wall clock) time.
  */
  i64 unloadInodeForPath(
    1: string mountPoint,
    2: string path,
    3: TimeSpec age,
  ) throws (1: EdenError ex)

  /**
   * Flush all thread-local stats to the main ServiceData object.
   *
   * Thread-local counters are normally flushed to the main ServiceData once
   * a second.  flushStatsNow() can be used to flush thread-local counters on
   * demand, in addition to the normal once-a-second flush.
   *
   * This is mainly useful for unit and integration tests that want to ensure
   * they see up-to-date counter information without waiting for the normal
   * flush interval.
   */
  void flushStatsNow() throws (1: EdenError ex)

  /**
  * Invalidate kernel cache for inode.
  */
  void invalidateKernelInodeCache(
    1: string mountPoint,
    2: string path
    )
  throws (1: EdenError ex)

 /**
   * Gets the number of inodes unloaded by periodic job on an EdenMount.
   */
  InternalStats getStatInfo() throws (1: EdenError ex)
}
