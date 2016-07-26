include "common/fb303/if/fb303.thrift"

namespace cpp2 facebook.eden
namespace java com.facebook.eden
namespace py facebook.eden

exception EdenError {
  1: required string message
  2: optional i32 errorCode
} (message = 'message')


struct MountInfo {
  1: string mountPoint
  2: string edenClientPath
}

service EdenService extends fb303.FacebookService {
  list<MountInfo> listMounts() throws (1: EdenError ex)
  void mount(1: MountInfo info) throws (1: EdenError ex)
  void unmount(1: string mountPoint) throws (1: EdenError ex)

  void checkOutRevision(1: string mountPoint, 2: string hash)
    throws (1: EdenError ex)

  // Mount-specific APIs.

  /**
   * Throws an EdenError if any of the following occur:
   * - path is the empty string.
   * - path identifies a non-existent file.
   * - path identifies something that is not an ordinary file (e.g., symlink
   *   or directory).
   */
  binary getSHA1(1: string mountPoint, 2: string path)
    throws (1: EdenError ex)
}
