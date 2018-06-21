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

/**
 * A source control hash.
 *
 * This should normally be a 20-byte binary value, however the edenfs server
 * will accept BinaryHash arguments as 40-byte hexadecimal strings as well.
 * Data returned by the edenfs server in a BinaryHash field will always be a
 * 20-byte binary value.
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
   * the union of the set of files whose ScmFileStatus differed from the
   * committed fromPosition hash before the hash changed, and the set of
   * files whose ScmFileStatus differed from the committed toPosition hash
   * after the hash was changed.  This list of files represents files
   * whose state may have changed as part of an update operation, but
   * in ways that may not be able to be extracted solely by performing
   * source control diff operations on the from/to hashes. */
  6: list<string> uncleanPaths
}

struct DebugGetRawJournalParams {
  1: string mountPoint
  2: JournalPosition fromPosition
  3: JournalPosition toPosition
}

struct DebugGetRawJournalResponse {
  1: list<FileDelta> deltas
}

/**
 * Classifies the change of the state of a file between and old and new state
 * of the repository. Most commonly, the "old state" is the parent commit while
 * the "new state" is the working copy.
 */
enum ScmFileStatus {
  /**
   * File is present in the new state, but was not present in old state.
   */
  ADDED = 0x0,

  /**
   * File is present in both the new and old states, but its contents or
   * file permissions have changed.
   */
  MODIFIED = 0x1,

  /**
   * File was present in the old state, but is not present in the new state.
   */
  REMOVED = 0x2,

  /**
   * File is present in the new state, but it is ignored according to the rules
   * of the new state.
   */
  IGNORED = 0x3,
}

struct ScmStatus {
  1: map<string, ScmFileStatus> entries

  /**
   * A map of { path -> error message }
   *
   * If any errors occured while computing the diff they will be reported here.
   * The results listed in the entries field may not be accurate for any paths
   * listed in this error field.
   *
   * This map will be empty if no errors occurred.
   */
  2: map<string, string> errors
}

/** Option for use with checkOutRevision(). */
enum CheckoutMode {
  /**
   * Perform a "normal" checkout, analogous to `hg checkout` in Mercurial. Files
   * in the working copy will be changed to reflect the destination snapshot,
   * though files with conflicts will not be modified.
   */
  NORMAL = 0,

  /**
   * Do not checkout: exercise the checkout logic to discover potential
   * conflicts.
   */
  DRY_RUN = 1,

  /**
   * Perform a "forced" checkout, analogous to `hg checkout --clean` in
   * Mercurial. Conflicts between the working copy and destination snapshot will
   * be forcibly ignored in favor of the state of the new snapshot.
   */
  FORCE = 2,
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

struct SetLogLevelResult {
  1: bool categoryCreated
}

/**
 * Struct to store Information about inodes in a mount point.
 */
struct MountInodeInfo {
  1: i64 loadedInodeCount
  2: i64 unloadedInodeCount
  3: i64 materializedInodeCount
  4: i64 loadedFileCount
  5: i64 loadedTreeCount
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
  /**
   * Linux-only: the contents of /proc/self/smaps, to be parsed by the caller.
   */
  4: binary smaps
  /**
   * Linux-only: privateBytes populated from contents of /proc/self/smaps.
   * Populated with current value (the fb303 counters value is an average).
   */
  5: i64 privateBytes
  /**
   * Linux-only: vmRSS bytes is populated from contents of /proc/self/stats.
   * Populated with current value (the fb303 counters value is an average).
   */
  6: i64 vmRSSBytes
}

struct ManifestEntry {
  /* mode_t */
  1: i32 mode
}

struct FuseCall {
  1: i32 len
  2: i32 opcode
  3: i64 unique
  4: i64 nodeid
  5: i32 uid
  6: i32 gid
  7: i32 pid
}

/** Params for globFiles(). */
struct GlobParams {
  1: string mountPoint,
  2: list<string> globs,
  3: bool includeDotfiles,
  // if true, prefetch matching blobs
  4: bool prefetchFiles,
  // if true, don't populate matchingFiles in the Glob
  // results.  This only really makes sense with prefetchFiles.
  5: bool suppressFileList,
}

struct Glob {
  /**
   * This list cannot contain duplicate values and is not guaranteed to be
   * sorted.
   */
  1: list<string> matchingFiles,
}

service EdenService extends fb303.FacebookService {
  list<MountInfo> listMounts() throws (1: EdenError ex)
  void mount(1: MountInfo info) throws (1: EdenError ex)
  void unmount(1: string mountPoint) throws (1: EdenError ex)

  /**
   * Potentially check out the specified snapshot, reporting conflicts (and
   * possibly errors), as appropriate.
   *
   * If the checkoutMode is FORCE, the working directory will be forcibly
   * updated to the contents of the new snapshot, even if there were conflicts.
   * Conflicts will still be reported in the return value, but the files will be
   * updated to their new state.
   *
   * If the checkoutMode is NORMAL, files with conflicts will be left
   * unmodified. Files that are untracked in both the source and destination
   * snapshots are always left unchanged, even if force is true.
   *
   * If the checkoutMode is DRY_RUN, then no files are modified in the working
   * copy and the current snapshot does not change. However, potential conflicts
   * are still reported in the return value.
   *
   * On successful return from this function (unless it is a DRY_RUN), the mount
   * point will point to the new snapshot, even if some paths had conflicts or
   * errors. The caller is responsible for taking appropriate action to update
   * these paths as desired after checkOutRevision() returns.
   */
  list<CheckoutConflict> checkOutRevision(
    1: string mountPoint,
    2: BinaryHash snapshotHash,
    3: CheckoutMode checkoutMode)
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

  /**
   * Returns the journal entries for the specified params. Useful for auditing
   * the changes that Eden has sent to Watchman. Note that the most recent
   * journal entries will be at the front of the list in
   * DebugGetRawJournalResponse.
   */
  DebugGetRawJournalResponse debugGetRawJournal(
    1: DebugGetRawJournalParams params,
  ) throws (1: EdenError ex)

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

  /**
   * DEPRECATED: Prefer globFiles().
   * Returns a list of files that match the input globs.
   * There are no duplicate values in the result.
   */
  list<string> glob(
    1: string mountPoint,
    2: list<string> globs)
      throws (1: EdenError ex)

  /**
   * Returns a list of files that match the GlobParams, notably,
   * the list of glob patterns.
   * There are no duplicate values in the result.
   */
  Glob globFiles(
    1: GlobParams params,
  ) throws (1: EdenError ex)

  /**
   * Get the status of the working directory against the specified commit.
   *
   * This may exclude special files according to the rules of the underlying
   * SCM system, such as the .git folder in Git and the .hg folder in Mercurial.
   */
  ScmStatus getScmStatus(
    1: string mountPoint,
    2: bool listIgnored,
    3: BinaryHash commit,
  ) throws (1: EdenError ex)

  /**
   * Computes the status between two specified revisions.
   * This does not care about the state of the working copy.
   */
  ScmStatus getScmStatusBetweenRevisions(
    1: string mountPoint,
    2: BinaryHash oldHash,
    3: BinaryHash newHash,
  ) throws (1: EdenError ex)

  //////// SCM Commit-Related APIs ////////

  /**
   * If the relative path exists in the manifest (i.e., the current commit),
   * then return the corresponding ManifestEntry; otherwise, throw
   * NoValueForKeyError.
   *
   * Note that we are still experimenting with the type of SCM information Eden
   * should be responsible for reporting, so this method is subject to change,
   * or may go away entirely. At a minimum, it should take a commit as a
   * parameter rather than assuming the current commit.
   */
  ManifestEntry getManifestEntry(
    1: string mountPoint
    2: string relativePath
  ) throws (
    1: EdenError ex
    2: NoValueForKeyError noValueForKeyError
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
   * Get the list of outstanding fuse requests
   *
   * This will return the list of FuseCall structure containing the data from
   * fuse_in_header.
   */
  list<FuseCall> debugOutstandingFuseCalls(
    1: string mountPoint,
  )

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
  SetLogLevelResult debugSetLogLevel(
    1: string category,
    2: string level,
  ) throws (1: EdenError ex)

  /**
   * Clears all data from the LocalStore that can be populated from the upstream
   * backing store.
   */
  void debugClearLocalStoreCaches() throws (1: EdenError ex)

  /**
   * Asks RocksDB to perform a compaction.
   */
  void debugCompactLocalStorage() throws (1: EdenError ex)

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

  /**
   * Ask the server to shutdown and provide it some context for its logs
   */
  void initiateShutdown(1: string reason);
}
