include "common/fb303/if/fb303.thrift"

namespace py scm.mononoke.apiserver.thrift.apiserver
namespace py3 scm.mononoke.apiserver.thrift

enum MononokeAPIExceptionKind {
  InvalidInput = 1,
  NotFound = 2,
  InternalError = 3,
  BookmarkNotFound = 4,
}

exception MononokeAPIException {
  1: MononokeAPIExceptionKind kind,
  2: string reason,
}

union MononokeRevision {
  1: string commit_hash,
  #Not yet supported, do not use
  2: string bookmark,
}

struct MononokeGetRawParams {
  1: string repo,
  2: MononokeRevision revision,
  3: binary path,
}

struct MononokeGetChangesetParams {
    1: string repo,
    3: MononokeRevision revision,
}

struct MononokeGetBranchesParams{
  1: string repo,
}

struct MononokeListDirectoryParams{
  1: string repo,
  2: MononokeRevision revision,
  3: binary path,
}

struct MononokeIsAncestorParams {
  1: string repo,
  2: MononokeRevision ancestor,
  3: MononokeRevision descendant,
}

struct MononokeChangeset {
  1: string commit_hash,
  2: string message,
  3: i64 date,
  4: string author,
  5: list<string> parents
  6: map<string, binary> extra,
}

struct MononokeBranches {
  1: map<string, string> branches,
}

struct MononokeDirectory {
  1: list<MononokeFile> files,
}

struct MononokeFile {
  1: string name,
  2: MononokeFileType file_type,
}

enum MononokeFileType {
  FILE = 0,
  TREE = 1,
  EXECUTABLE = 2,
  SYMLINK = 3,
}

service MononokeAPIService extends fb303.FacebookService {
  binary get_raw(1: MononokeGetRawParams params)
    throws (1: MononokeAPIException e),

  MononokeChangeset get_changeset(1: MononokeGetChangesetParams param)
    throws (1: MononokeAPIException e),

  MononokeBranches get_branches(1: MononokeGetBranchesParams params)
    throws (1: MononokeAPIException e),

  MononokeDirectory list_directory(1: MononokeListDirectoryParams params)
    throws (1: MononokeAPIException e),

  bool is_ancestor(1: MononokeIsAncestorParams params)
    throws (1: MononokeAPIException e),
}
