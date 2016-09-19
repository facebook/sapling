include "common/fb303/if/fb303.thrift"

namespace cpp2 facebook.eden
namespace java com.facebook.eden.thrift
namespace py facebook.eden

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
  1: i64 size        // wish thrift had unsigned numbers
  2: TimeSpec mtime
  3: i32 mode        // mode_t
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
  map<string, FileInformation> getMaterializedEntries(1: string mountPoint)
}
