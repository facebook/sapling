namespace cpp2 facebook.eden.hgdirstate
namespace java com.facebook.eden.hgdirstate
namespace py facebook.hgdirstate

typedef string RelativePath

// Note that if the dirstate sets the status to 'n', then the file should no
// longer have an entry in the DirstateNonnormalFiles map.
enum DirstateNonnormalFileStatus {
  // Ideally, there would not be any entries marked `Normal` in the
  // DirstateNonnormalFiles map. However, Mercurial sometimes marks a file as
  // `Normal` but with DirstateMergeState.BothParents or
  // DirstateMergeState.OtherParent, so we must include this value in the enum.
  Normal = 0x0,             // 'n'

  // This can also be used with DirstateMergeState.OtherParent.
  NeedsMerging = 0x1,       // 'm'

  // In this state, a file may also be marked as DirstateMergeState.BothParents
  // or DirstateMergeState.OtherParent.
  MarkedForRemoval = 0x2,   // 'r'

  MarkedForAddition = 0x3,  // 'a'

  NotTracked = 0x4,         // '?'
}

enum DirstateMergeState {
  NotApplicable = 0x0, // >= 0
  BothParents = 0x1,   // -1
  OtherParent = 0x2,   // -2
}

/**
 * Represents two of the four fields in a dirstatetuple in Mercurial. The four
 * fields are: status, mode, size, mtime. The mapping is as follows:
 *
 * - status: DirstateNonnormalFileStatus.
 * - mode: Fetch the st_mode from Eden.
 * - size: DirstateMergeState. We are not interested in Hg storing the actual
 *   size of the file, but we are interested in the case where it sets it to
 *   the special values -1 and -2, which is captured by the DirstateMergeState
 *   enum.
 * - mtime: Fetch the mtime from Eden, if necessary, though we should eliminate
 *   all codepaths in Hg that read this field. Note that Mercurial sometimes
 *   sets the mtime to -1 as a hack to indicate that the file status is unknown
 *   and needs to be recalculated. Eden should never be in this indeterminate
 *   state, so if we see -1, we should either throw or ignore it.
 *
 * Unfortunately, not every (DirstateNonnormalFileStatus, DirstateMergeState)
 * pair is a valid state in Hg, so we have to throw a runtime exception if we
 * encounter value for this struct that we deem to be illegitimate.
 */
struct DirstateNonnormalFile {
  1: DirstateNonnormalFileStatus status,
  2: DirstateMergeState mergeState,  // 'size' in Hg's native dirstate.
}

// Key is a repo-relative file path. Value is the corresponding metadata.
struct DirstateNonnormalFiles {
  1: map<RelativePath, DirstateNonnormalFile> entries
}

// Keys and values are both repo-relative file paths.
// Keys are destinations whereas values are sources.
struct DirstateCopymap {
  1: map<RelativePath, RelativePath> entries
}

struct DirstateTuple {
  1: DirstateNonnormalFileStatus status,
  2: i32 mode
  3: DirstateMergeState mergeState,
}
