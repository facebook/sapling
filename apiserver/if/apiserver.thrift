/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

include "common/fb303/if/fb303.thrift"

namespace cpp2 scm.mononoke.apiserver.thrift
namespace py scm.mononoke.apiserver.thrift.apiserver
namespace py3 scm.mononoke.apiserver.thrift

typedef binary (cpp2.type = "std::unique_ptr<folly::IOBuf>") IOBufPointer

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

struct MononokeNodeHash {
  1: string hash,
}

struct MononokeTreeHash {
  1: string hash,
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

struct MononokeGetFileHistoryParams{
  1: string repo,
  2: MononokeRevision revision,
  3: binary path,
  4: i32 limit,
  5: i32 skip,
}

struct MononokeGetLastCommitOnPathParams{
  1: string repo,
  2: MononokeRevision revision,
  3: binary path,
}

struct MononokeListDirectoryParams{
  1: string repo,
  2: MononokeRevision revision,
  3: binary path,
}

struct MononokeListDirectoryUnodesParams{
  1: string repo,
  2: MononokeRevision revision,
  3: binary path,
}

struct MononokeIsAncestorParams {
  1: string repo,
  2: MononokeRevision ancestor,
  3: MononokeRevision descendant,
}

struct MononokeGetBlobParams {
  1: string repo,
  2: MononokeNodeHash blob_hash,
}

struct MononokeGetTreeParams {
  1: string repo,
  2: MononokeTreeHash tree_hash,
}

struct MononokeChangeset {
  1: string commit_hash,
  2: string message,
  3: i64 date,
  4: string author,
  5: list<string> parents
  6: map<string, binary> extra,
  7: MononokeTreeHash manifest,
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
  3: MononokeNodeHash hash,
  4: optional i64 size,
  5: optional string content_sha1,
}

struct MononokeFileHistory {
  1: list<MononokeChangeset> history,
}

struct MononokeDirectoryUnodes {
  1: list<MononokeEntryUnodes> entries,
}

struct MononokeEntryUnodes {
  1: string name,
  2: bool is_directory,
}

struct MononokeBlob {
  1: IOBufPointer content,
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

  MononokeFileHistory get_file_history(1: MononokeGetFileHistoryParams params)
    throws (1: MononokeAPIException e),

  MononokeChangeset get_last_commit_on_path(1: MononokeGetLastCommitOnPathParams params)
    throws (1: MononokeAPIException e),

  # Having two different list_directory methods is a temporary state
  # until we get unodes deployed everywhere.
  MononokeDirectory list_directory(1: MononokeListDirectoryParams params)
    throws (1: MononokeAPIException e),

  MononokeDirectoryUnodes list_directory_unodes(1: MononokeListDirectoryUnodesParams params)
    throws (1: MononokeAPIException e),

  bool is_ancestor(1: MononokeIsAncestorParams params)
    throws (1: MononokeAPIException e),

  MononokeBlob get_blob(1: MononokeGetBlobParams params)
    throws (1: MononokeAPIException e),

  MononokeDirectory get_tree(1: MononokeGetTreeParams params)
    throws (1: MononokeAPIException e),
}
