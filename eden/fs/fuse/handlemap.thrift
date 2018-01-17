namespace cpp2 facebook.eden

struct FileHandleMapEntry {
  1: i64 inodeNumber
  2: i64 handleId // really u64
  3: bool isDir
}

struct SerializedFileHandleMap {
  1: list<FileHandleMapEntry> entries
}

