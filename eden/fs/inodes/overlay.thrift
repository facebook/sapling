include "eden/fs/inodes/hgdirstate.thrift"

namespace cpp2 facebook.eden.overlay
namespace py facebook.eden.overlay

typedef binary Hash
typedef string PathComponent
typedef string RelativePath

struct OverlayEntry {
  // Holds the mode_t data, which encodes the file type and permissions
  1: i32 mode
  // The inodeNumber of the child, if it is materialized.
  // If the child is not materialized this will be 0, and the hash will
  // contain the hash of a source control Tree or Blob.
  2: i64 inodeNumber
  // If inodeNumber is 0, then this child is identical to an existing
  // source control Tree or Blob.  This contains the hash of that Tree or Blob.
  3: Hash hash
}

struct OverlayDir {
  // The contents of this dir.
  1: map<PathComponent, OverlayEntry> entries
  // For a placeholder entry that is materialized in name only (for example,
  // renaming a directory without materializing the entire content),
  // the key that we will use to load the source TreeEntry when needed
  2: Hash treeHash
}

struct OverlayData {
  // A map of RelativePath -> OverlayDir for the entire contents of the
  // overlay area.  The assumption is that the locally materialized data
  // (since it should be O(things-changed-in-1-diff) should reasonably
  // fit in memory and thus that this won't be too big to work with.
  1: map<RelativePath, OverlayDir> localDirs
}

enum UserStatusDirective {
  Add = 0x0,
  Remove = 0x1,
}

struct DirstateData {
  1: map<RelativePath, hgdirstate.DirstateTuple> hgDirstateTuples
  2: map<RelativePath, RelativePath> hgDestToSourceCopyMap
}
