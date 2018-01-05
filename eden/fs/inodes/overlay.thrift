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
}
