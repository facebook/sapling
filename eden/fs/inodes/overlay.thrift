namespace cpp2 facebook.eden.overlay

typedef binary Hash
typedef string PathComponent
typedef string RelativePath

struct OverlayEntry {
  // Holds the mode_t data, which encodes the file type and permissions
  1: i32 mode
  // For a placeholder entry that is materialized in name only (not content),
  // this is either the Tree hash or Blob hash (depending on whether this is
  // a directory or file) that we can use to obtain the content on demand.
  // If this is null then the entry was not based on data available in the tree
  // (eg: a newly created file or dir).
  2: Hash hash
  3: bool materialized
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
