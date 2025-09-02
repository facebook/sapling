/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "fb303/thrift/fb303_core.thrift"
include "thrift/annotation/thrift.thrift"
include "eden/mononoke/megarepo_api/if/megarepo_configs.thrift"
include "eden/mononoke/derived_data/if/derived_data_type.thrift"
include "thrift/annotation/rust.thrift"
include "thrift/annotation/hack.thrift"

namespace cpp2 facebook.scm.service
namespace php SourceControlService
namespace py scm.service.thrift.source_control
namespace py3 scm.service.thrift
namespace java.swift com.facebook.scm.service

@rust.Type{name = "Bytes"}
typedef binary binary_bytes
@rust.NewType
@rust.Type{name = "smallvec::SmallVec<[u8; 24]>"}
typedef binary small_binary
typedef derived_data_type.DerivedDataType DerivedDataType

struct DateTime {
  /// UNIX timestamp
  1: i64 timestamp;
  /// Time zone offsets in seconds
  2: i32 tz;
}

/// Specifiers
///
/// Specifiers are used in each call to identify entities within source control.

@rust.Ord
struct RepoSpecifier {
  /// The name of the repository.
  1: string name;
}

/// The schemes by which commits can be identified.
enum CommitIdentityScheme {
  UNKNOWN = 0,

  /// Commits are identified by the 32-byte hash of Mononoke's bonsai
  /// changeset.
  BONSAI = 1,

  /// Commits are identified by the 20-byte hash of the Mercurial commit.
  HG = 2,

  /// Commits are identified by the 20-byte hash of the Git commit.
  GIT = 3,

  /// Commits are identified by an externally-assigned repo-wide unique
  /// integer.
  GLOBALREV = 4,

  /// Commits identified by integer svn revision number.
  SVNREV = 5,

  /// 32-byte hash of Mononoke's bonsai changeset, plus a 64-bit bubble
  EPHEMERAL_BONSAI = 6,
}

/// A commit stored in an ephemeral bubble
struct EphemeralBonsai {
  1: binary bonsai_id;

  /// If 0, means the bubble is unknown and should be fetched
  2: i64 bubble_id;
}

/// A unique identifier for a commit.
///
/// Commit hashes are represented using raw binary, not as hex-encoded strings.
/// If you have a hex-encoded string hash you must convert it to binary, for
/// example using:
///
///    - In Rust: `faster_hex::hex_decode`
///    - In Python: `bytes.fromhex`
///    - In PHP/Hack: `Str::hex2bin`
@hack.MigrationBlockingLegacyJSONSerialization
@rust.Ord
union CommitId {
  /// Commit identified by the hash of Mononoke's bonsai changeset.
  1: binary bonsai;

  /// Commit identified by the hash of the Mercurial commit.
  2: binary hg;

  /// Commit identified by the hash of the Git commit.
  3: binary git;

  /// Commit identified by an externally-assigned repo-wide unique integer.
  4: i64 globalrev;

  /// Commit identified by svn revision number.
  5: i64 svnrev;

  /// Bonsai commit stored in an ephemeral bubble
  6: EphemeralBonsai ephemeral_bonsai;
}

/// Specified a commit within a repo.
@rust.Ord
struct CommitSpecifier {
  /// The repository that contains the commit.
  1: RepoSpecifier repo;

  2: CommitId id;
}

/// The UTF-8 path of the file or directory.
typedef string Path

/// Specifies a file or directory within a commit.
struct CommitPathSpecifier {
  /// The commit within which the file or directory is found.
  1: CommitSpecifier commit;

  /// The UTF-8 path of the file or directory.
  2: Path path;
}

/// Specifies a tree by its ID.
struct TreeIdSpecifier {
  /// The repository that contains the tree.
  1: RepoSpecifier repo;

  /// The ID of the tree, obtained from a previous call to the service.
  2: binary id;
}

@hack.MigrationBlockingLegacyJSONSerialization
union TreeSpecifier {
  /// Specify a tree by its path in a commit.
  1: CommitPathSpecifier by_commit_path;

  /// Specify a tree by its id.
  2: TreeIdSpecifier by_id;
}

/// Specifies a file by its ID.
struct FileIdSpecifier {
  /// The repository that contains the file.
  1: RepoSpecifier repo;

  /// The ID of the file, obtained from a previous call to the service.
  2: binary id;
}

/// Specifies a file by its content hash.
struct FileContentHashSpecifier {
  /// The repository that contains the file.
  1: RepoSpecifier repo;

  /// The content hash of the file.
  2: binary content_hash;
}

@hack.MigrationBlockingLegacyJSONSerialization
union FileSpecifier {
  /// Specify a file by its path in a commit.
  1: CommitPathSpecifier by_commit_path;

  /// Specify a file by its id.
  2: FileIdSpecifier by_id;

  /// Specify a file by its SHA-1 content hash.
  3: FileContentHashSpecifier by_sha1_content_hash;

  /// Specify a file by its SHA-256 content hash.
  4: FileContentHashSpecifier by_sha256_content_hash;
}

/// Returned objects

/// This structure should be small and contain very basic repository info.
struct Repo {
  1: string name;
}

/// This structure can be bigger and contain more detailed repository info.
struct RepoInfo {
  1: string name;
  2: CommitIdentityScheme default_commit_identity_scheme;
  /// Name of a large repo to which this repo is push redirected, i.e. when
  /// the large repo is the source of truth.
  3: optional string push_redirected_to;
}

struct CommitInfo {
  /// The IDs of the commit in the requested identity schemes.
  2: map<CommitIdentityScheme, CommitId> ids;

  /// The commit message.
  3: string message;

  /// The date the commit was authored.
  4: i64 date;

  /// The author of the commit.
  5: string author;

  /// The parents of the commit, in the requested identity schemes.
  6: list<map<CommitIdentityScheme, CommitId>> parents;

  /// Extra metadata about the commit.
  7: map<string, binary> extra;

  /// The timezone the commit was authored in, in seconds after UTC.
  8: i32 tz;

  /// Length of longest path between this commit and any root.
  9: i64 generation;

  /// Extra git headers associated with the commit if the commit is a
  /// mirrored version from a git repo.
  10: optional map<small_binary, binary_bytes> git_extra_headers;

  /// The date the commit was committed (if available - commit comes from Git).
  11: optional i64 committer_date;

  /// The identity of the person who committed this commit, as opposed to authored it (if available - commit comes from Git).
  12: optional string committer;

  /// The linear depth of the commit. It's calculated as the number of ancestors of the commit if the commit
  /// graph consisted only of the first parents (i.e. if merges were ignored).
  ///
  /// This can be useful when using the commit_linear_history method. For example commit_linear_history(commit.id, skip=commit.linear_depth, limit=1)
  /// will return the root commit of the repository.
  13: optional i64 linear_depth;

  /// The timezone the commit was committed in, in seconds after UTC.
  14: optional i32 committer_tz;

  /// The number of subtree changes in the commit.
  15: optional i64 subtree_change_count;
}

struct BookmarkInfo {
  /// "Warm" bookmark value. That's the value of the bookmark that would be
  /// provided on any other query (like repo_resolve_bookmark).  For the warm
  /// value all the data like history or blame data is precomputed.
  1: map<CommitIdentityScheme, CommitId> warm_ids;
  /// "Real" bookmark value. This is the actual value of a bookmark. Maybe be
  /// slightly stale (as the read is coming from local mysql replica).
  2: map<CommitIdentityScheme, CommitId> fresh_ids;
  /// The timestamp of the last update. This is the update time when the "fresh"
  /// value provided was set.
  3: i64 last_update_timestamp_ns;
}

enum EntryType {
  /// Unknown type
  UNKNOWN = 0,

  /// Normal file
  FILE = 1,

  /// Executable file
  EXEC = 2,

  /// Symbolic link
  LINK = 3,

  /// Sub-directory
  TREE = 4,

  /// Git submodule
  GIT_SUBMODULE = 5,
}

struct FileInfo {
  /// The id of the file contents that can be used in subsequent look-ups.
  1: binary id;

  /// The size of the file, or the length of the link target path for links.
  2: i64 file_size;

  /// The content sha1 of the file.
  3: binary content_sha1;

  /// The content sha256 of the file.
  4: binary content_sha256;

  /// Git SHA1 hash of the content of the file
  5: binary content_git_sha1;

  /// If this file is a binary file
  6: bool is_binary;

  /// If the file is ASCII encoded
  7: bool is_ascii;

  /// If the file is UTF-8 encoded
  8: bool is_utf8;

  /// If the file is ends with a new line
  9: bool ends_in_newline;

  /// How many newlines does this file has
  10: i64 newline_count;

  /// The first UTF-8 encoded line of the file content,
  /// or UTF-8 string equivalent of the first 64 bytes,
  /// whichiever is shorter. This field is None if the file
  /// is not encoded with UTF-8
  11: optional string first_line;

  /// Is the file auto-generated
  /// i.e., contains the '@ generated' tag ?
  12: bool is_generated;

  /// Is the file partially-generated
  /// i.e., contains the '@ partially-generated' tag ?
  13: bool is_partially_generated;

  /// Blake3 hash of the file seeded with the global thrift
  /// constant in fbcode/blake3.thrift
  14: binary content_seeded_blake3;
}

struct TreeInfo {
  /// The id of the tree that can be used in subsequent look-ups.
  1: binary id;

  /// DEPRECATED: The sha1 of the simple format of the directory.
  2: optional binary simple_format_sha1;

  /// DEPRECATED: The sha256 of the simple format of the directory.
  3: optional binary simple_format_sha256;

  /// The count of files inside the directory (excluding files inside
  /// subdirectories).
  4: i64 child_files_count;

  /// The total size of the files inside the directory (excluding files
  /// inside subdirectories).
  5: i64 child_files_total_size;

  /// The count of subdirectories inside the directory (excluding
  /// directories inside subdirectories).
  6: i64 child_dirs_count;

  /// The count of all files in the directory (including files in
  /// subdirectories).
  7: i64 descendant_files_count;

  /// The total size of all files in the directory (including files in
  /// subdirectories).
  8: i64 descendant_files_total_size;
}

@hack.MigrationBlockingLegacyJSONSerialization
union EntryInfo {
  1: TreeInfo tree;
  2: FileInfo file;
}

struct TreeEntry {
  /// The name of the entry in this directory.
  1: string name;

  /// The type of the entry (file, link, exec, or sub-directory)
  2: EntryType type;

  /// The info for the entry (file or sub-directory).
  3: EntryInfo info;
}

struct FilePathInfo {
  /// The repo-root relative path.
  1: string path;

  /// The type of the entry (file, link, exec)
  2: EntryType type;

  /// The info for the entry.
  3: FileInfo info;
}

struct TreePathInfo {
  /// The repo-root relative path.
  1: string path;

  /// The info for the entry.
  2: TreeInfo info;
}

struct FileChunk {
  /// The offset within the file for this chunk.
  1: i64 offset;

  /// The total size of the file.
  2: i64 file_size;

  /// The data for this chunk.
  3: binary data;
}

/// If a file or tree was copied via a subtree change, this will contain the original commit and path
/// that it was copied from.
struct CommitCompareSubtreeSource {
  /// Commit id for the "other" file if it came from a different location than in the request.
  /// This may happen if the file was copied or merged due to a subtree change.
  4: map<CommitIdentityScheme, CommitId> source_commit_ids;
  /// Path for the "other" file if it came from a different location than in the request.
  /// This may happen if the file was copied or merged due to a subtree change.
  5: Path source_path;
}

struct CommitCompareFile {
  1: optional FilePathInfo base_file;
  2: optional FilePathInfo other_file;

  /// Whether the file was copied or moved.  This is different than NONE only when a commit is compared with its parent
  3: CopyInfo copy_info;

  /// When a file was copied by a subtree copy, this contains the original commit and path that it was copied from.
  /// Set only when compared with a parent and when compare_against_subtree_copy_sources is set to true.
  4: optional CommitCompareSubtreeSource subtree_source;
}

struct CommitCompareTree {
  1: optional TreePathInfo base_tree;
  2: optional TreePathInfo other_tree;

  /// Whether the file was copied or moved.  This is different than NONE only when a commit is compared with its parent
  3: CopyInfo copy_info;

  /// When a file was copied by a subtree copy, this contains the original commit and path that it was copied from.
  /// Set only when compared with a parent and when compare_against_subtree_copy_sources is set to true.
  4: optional CommitCompareSubtreeSource subtree_source;
}

enum CommitCompareItem {
  FILES = 0,
  TREES = 1,
}

/// The formats in which we can render the diff.
/// Just one now, but we want to return more structured diffs in the future.
enum DiffFormat {
  /// Raw diff (unified diff format with some of the "git diff" improvements)
  RAW_DIFF = 0,
  /// Metadata diff (file types, summaries of added and removed lines, etc.)
  METADATA_DIFF = 1,
}

/// The formats in which we can render the diff.
@hack.MigrationBlockingLegacyJSONSerialization
union Diff {
  1: RawDiff raw_diff;
  2: MetadataDiff metadata_diff;
}

/// Raw diff (unified diff format with some of the "git diff" improvements).
struct RawDiff {
  /// Raw diff as bytes.
  1: optional binary raw_diff;
  /// One of the files is binary, raw diff contains just a placeholder.
  2: bool is_binary;
}

/// Metadata diff (file types, summaries of added and removed lines, etc.).
struct MetadataDiff {
  /// Information about the file before the change.
  5: MetadataDiffFileInfo old_file_info;

  /// Information about the file after the change.
  6: MetadataDiffFileInfo new_file_info;

  /// Lines count in the diff between the two files.
  7: optional MetadataDiffLinesCount lines_count;
}

/// File information that concerns the metadata diff.
struct MetadataDiffFileInfo {
  /// File type (file, exec, or link)
  1: optional MetadataDiffFileType file_type;

  /// File content type (text, non-utf8, or binary)
  2: optional MetadataDiffFileContentType file_content_type;

  /// File generated status (fully, partially, or not generated)
  3: optional FileGeneratedStatus file_generated_status;
}

enum MetadataDiffFileType {
  /// An ordinary file (equivalent to mode "100644")
  FILE = 1,

  /// An executable file (equivalent to mode "100755")
  EXEC = 2,

  /// A symbolic link (equivalent to mode "120000")
  LINK = 3,

  /// A git submodule
  GIT_SUBMODULE = 4,
}

enum MetadataDiffFileContentType {
  /// File content is entirely valid UTF-8 text
  TEXT = 1,

  /// File content contains no NUL bytes, but is not valid UTF-8
  NON_UTF8 = 2,

  /// File content includes NUL bytes, thus is likely to be binary
  BINARY = 3,
}

enum FileGeneratedStatus {
  /// File is fully generated.
  FULLY_GENERATED = 1,

  /// File is partially generated (contains manual sections)
  PARTIALLY_GENERATED = 2,

  /// File is not generated.
  NOT_GENERATED = 3,
}

/// Lines count in a diff.
struct MetadataDiffLinesCount {
  /// Number of added lines.
  1: i64 added_lines_count;

  /// Number of deleted lines.
  2: i64 deleted_lines_count;

  /// Number of significant (not generated) lines.
  3: i64 significant_added_lines_count;

  /// Number of significant (not generated) lines.
  4: i64 significant_deleted_lines_count;

  /// Line number of the first added line (1-based).
  5: optional i64 first_added_line_number;
}

/// Indicates whether the file was copied or moved
enum CopyInfo {
  /// File was modified, added or removed.
  NONE = 0,
  /// File was moved.
  MOVE = 1,
  /// File was copied.
  COPY = 2,
}

enum BlameFormat {
  /// Use the BlameCompact format.
  COMPACT = 1,
}

enum BlameFormatOption {
  /// Applies to BlameCompact.  Controls whether the blame includes the content
  /// of each line.
  INCLUDE_CONTENTS = 1,

  /// Applies to BlameCompact.  Controls whether the blame includes the titles
  /// (first line or 128 characters of the commit message) of the commits that
  /// introduced each line.
  INCLUDE_TITLE = 2,

  /// Applies to BlameCompact.  Controls whether the blame includes the messages
  /// of the commits that introduced each line.
  INCLUDE_MESSAGE = 3,

  /// Applies to BlameCompact.  Controls whether the blame includes parent range
  /// information, i.e. which lines the blamed line is deemed to have replaced,
  /// and the parent commit identities for every commit.
  INCLUDE_PARENT = 4,

  /// Applies to BlameCompact.  Controls whether the blame includes per-commit
  /// numerical identifiers.  These identifiers are only valid within this
  /// blame instance, however attempts are made to keep these stable over the
  /// main (p1) history of the file.
  INCLUDE_COMMIT_NUMBERS = 5,
}

@hack.MigrationBlockingLegacyJSONSerialization
union Blame {
  2: BlameCompact blame_compact;
}

/// Compact blame format uses look-up tables for items that might be repeated
/// across the file.  Use the `_index` field in `BlameCompactLine` to index the
/// corresponding list in the `BlameCompact` to get the item for each line.
///
/// Some items might be missing, depending on the options selected in the
/// request.  In this case, the corresponding look-up table will also be
/// omitted.
///
/// ## Implementing "skip past this change"
///
/// The change that a line is blamed against can be skipped, i.e. the user
/// can be directed to the code *before* the change, by following these
/// steps:
///
/// * Use `parent_commit_ids[line.commit_id_index][line.parent_index]` as
///   the commit.
///
/// * If `parent_path_index` is present, use `paths[line.parent_path_index]`
///   as the path, otherwise use `paths[line.path_index]` as the path.
///
/// * Use `parent_start_line` and `parent_range_length` as the line range.
///   If the range length is 0, then the line was inserted *before* the
///   start line.
///
/// If none of the `parent_*` fields are  present then this line is an
/// original line from the first version of the file.
struct BlameCompact {
  1: list<BlameCompactLine> lines;
  2: list<map<CommitIdentityScheme, CommitId>> commit_ids;
  3: list<Path> paths;
  4: list<string> authors;
  5: list<DateTime> dates;
  6: optional list<string> titles;
  7: optional list<string> messages;

  /// The parent commit ids for each of the commits (in the same order as
  /// `commit_ids`).  Only present if `INCLUDE_PARENT` was requested.
  8: optional list<list<map<CommitIdentityScheme, CommitId>>> parent_commit_ids;

  /// Small numbers suitable for use to identify each of the commits within this
  /// blame (in the same order as `commit_ids`).  These numbers are not
  /// guaranteed to apply to the same commit in any other blame, however the
  /// server will attempt to keep the numbers stable over the main history of
  /// the file.
  9: optional list<i32> commit_numbers;

  /// Approximate number of commits in the history of this file (merged
  /// ancestors may not be included in this count if they did not contribute
  /// content to the merge).  Only present if `INCLUDE_COMMIT_NUMBERS` was
  /// requested.
  10: optional i32 approx_commit_count;

  /// Number of distinct ranges in the blame.
  11: optional i32 distinct_range_count;

  /// The parent commit ids for replacement parents (if any commit parent
  /// was replaced by a mutable rename or subtree copy).  Only present if
  /// `INCLUDE_PARENT` was requested.
  12: optional map<
    i32,
    map<CommitIdentityScheme, CommitId>
  > replacement_parent_commit_ids;
}

struct BlameCompactLine {
  /// The current line number of this line.
  1: i32 line;

  /// The content of this line.  This is only provided if `format_options`
  /// included `INCLUDE_CONTENTS` in the request.
  2: optional string contents;

  /// The index in the look-up table of the commit ID that introduced the line.
  3: i32 commit_id_index;

  /// The index in the look-up table of the path of the file when this line
  /// was introduced.
  4: i32 path_index;

  /// The index in the look-up table of the author that introduced this line.
  5: i32 author_index;

  /// The index in the look-up table of the date when this line was introduced.
  6: i32 date_index;

  /// The original line number when this line was introduced.
  7: i32 origin_line;

  /// The index in the look-up table of the title (first line or 128 characters
  /// of the commit message, whichever is shorter) of the commit that introduced
  /// this line.  This is only provided if `format_options` included
  /// `INCLUDE_TITLE` in the request.
  8: optional i32 title_index;

  /// The index in the look-up table of the message of the commit that
  /// introduced this line.  This is only provided if `format_options` included
  /// `INCLUDE_MESSAGE` in the request.
  9: optional i32 message_index;

  /// The index of the parent in the bonsai changeset's parents that this line
  /// is deemed to have replaced.  This is only provided if `format_options`
  /// included `INCLUDE_PARENT` in the request, and if the line is not from the
  /// original version of the file.  Use in conjunction with `commit_id_index`
  /// to find the parent commit from the `parent_commit_ids` look-up table.
  10: optional i32 parent_index;

  /// The line number in the parent of the start of the range that this
  /// line is deemed to have replaced.  This may be one greater than the number
  /// of lines in the file if the new lines were inserted at the end. This is
  /// only provided if `format_options` included `INCLUDE_PARENT` in the
  /// request, and if the lines is not from the original version of the file.
  11: optional i32 parent_start_line;

  /// The number of lines in the parent of the range that this line is deemed
  /// to have replaced.  If zero, then this line was inserted *before* the
  /// parent_start_line.  This is only provided if `format_options` included
  /// `INCLUDE_PARENT` in the request, and if the line is not from the original
  /// version of the file.
  12: optional i32 parent_range_length;

  /// If the file was renamed, the index into `paths` of the name of the
  /// file in the parent.  This is only provided if `format_options` included
  /// `INCLUDE_PARENT` in the request.
  13: optional i32 parent_path_index;

  /// May be set as an alternative to `parent_index` if the parent is a
  /// replacement parent (e.g. due to mutable rename or subtree copy).  In
  /// this case, it will be the index in the map of `replacement_parents`.
  /// This is only provided if `format_options` included `INCLUDE_PARENT` in
  /// the request.
  14: optional i32 replacement_parent_index;
}

enum HistoryFormat {
  COMMIT_INFO = 1,
  COMMIT_ID = 2,
}

enum MutationHistoryFormat {
  COMMIT_ID = 1,
}

@hack.MigrationBlockingLegacyJSONSerialization
union History {
  1: list<CommitInfo> commit_infos;
  2: list<map<CommitIdentityScheme, CommitId>> commit_ids;
}

struct PushrebaseRebasedCommit {
  /// The old commit id in the requested schemes.  This uses
  /// old_identity_schemes if specified in the request.
  1: map<CommitIdentityScheme, CommitId> old_ids;

  /// The new commit id in the requested schemes.
  2: map<CommitIdentityScheme, CommitId> new_ids;
}

struct PushrebaseOutcome {
  /// The new id for the rebased head.
  1: map<CommitIdentityScheme, CommitId> head;

  /// List of mappings from old commit id to new commit id for the rebased
  /// commits.  The identity schemes for the old ID is specified by the
  /// old_identity_schemes field in the request.
  2: list<PushrebaseRebasedCommit> rebased_commits;

  /// How far away was the commit rebased.
  3: i64 pushrebase_distance;

  /// How many retries it took to do the rebase successfully, due to race conditions.
  4: i64 retry_num;

  /// The old id where the bookmark was before the pushrebase operation.
  5: map<CommitIdentityScheme, CommitId> old_bookmark_value;
}

typedef string SparseProfileName

struct AllSparseProfiles {}

/// Which sparse profiles should be analysed
@hack.MigrationBlockingLegacyJSONSerialization
union SparseProfiles {
  /// Given list of names
  1: list<SparseProfileName> profiles;
  /// All available profiles based on configured root path
  2: AllSparseProfiles all_profiles;
}

struct SparseProfileAdded {
  // positive size of added profile
  1: i64 size;
}

struct SparseProfileRemoved {
  // positive size of removed profile
  1: i64 previous_size;
}

struct SparseProfileSizeChanged {
  // signed change in size
  1: i64 size_change;
}

@hack.MigrationBlockingLegacyJSONSerialization
union SparseProfileChangeElement {
  1: SparseProfileAdded added;
  2: SparseProfileRemoved removed;
  3: SparseProfileSizeChanged changed;
}

struct SparseProfileSize {
  1: i64 size;
}

struct SparseProfileChange {
  1: SparseProfileChangeElement change;
}

struct SparseProfileSizes {
  1: map<SparseProfileName, SparseProfileSize> sizes;
}

struct SparseProfileDeltaSizes {
  1: map<SparseProfileName, SparseProfileChange> size_changes;
}

/// Method parameters structures

struct RepoInfoParams {}

struct ListReposParams {}

struct RepoResolveBookmarkParams {
  /// The bookmark name to look up.
  1: string bookmark_name;

  /// Commit identity schemes to return.
  2: set<CommitIdentityScheme> identity_schemes;
}

struct RepoResolveCommitPrefixParams {
  /// The commit hash prefix to look up.
  1: string prefix;

  /// Identity scheme of the given prefix.
  2: CommitIdentityScheme prefix_scheme;

  /// Commit identity schemes to return.
  3: set<CommitIdentityScheme> identity_schemes;
}

struct RepoBookmarkInfoParams {
  /// The bookmark name to look up.
  1: string bookmark_name;

  /// Commit identity schemes to return.
  /// Note: ask for bonsai hash only for lowest latency.  The hg hash (and
  /// possibly others) are generated after the bookmark is moved and you might
  /// need to wait for them.
  2: set<CommitIdentityScheme> identity_schemes;
}

const i64 REPO_LIST_BOOKMARKS_MAX_LIMIT = 10000;

struct RepoListBookmarksParams {
  /// If true, include scratch bookmarks. To list scratch bookmarks, you
  /// must provide a non-empty prefix and a limit.
  1: bool include_scratch;

  /// Prefix to match when listing bookmarks.
  2: string bookmark_prefix;

  /// Limit to the number of bookmarks that may match.
  3: i64 limit;

  /// Return bookmarks after this name, to be used for paging.
  4: optional string after;

  /// Commit identity schemes to return.
  5: set<CommitIdentityScheme> identity_schemes;
}

const i64 REPO_STACK_INFO_MAX_LIMIT = 10000;

struct RepoStackInfoParams {
  /// Commit identity schemes to return.
  1: set<CommitIdentityScheme> identity_schemes;

  /// List of heads to generate stack for.
  2: list<CommitId> heads;

  /// Limits the number of draft changesets in the response, can be set up to
  /// REPO_STACK_INFO_MAX_LIMIT.
  3: i64 limit;
}

/// The parameter for the repo_stack_git_bundle_store method
struct RepoStackGitBundleStoreParams {
  /// The changeset ID of the commit which is the head of the stack of draft commits
  1: CommitId head;
  /// The changeset ID of the base commit (public or draft) that serves as the base
  /// of the stack of commits and is present in the user repo, i.e. the repo that will
  /// unbundle this bundle, already has this commit
  2: CommitId base;
  /// The identity of the service making the repo_stack_git_bundle_store request.
  3: optional string service_identity;
}

enum RepoCreateCommitParamsFileType {
  /// Normal file
  FILE = 1,

  /// Executable file
  EXEC = 2,

  /// Symbolic link
  LINK = 3,

  /// Git submodule
  GIT_SUBMODULE = 4,
}

@hack.MigrationBlockingLegacyJSONSerialization
union RepoCreateCommitParamsFileContent {
  /// Create the file using the provided data.
  1: binary data;

  /// Create the file using a pre-existing file specified by id.
  2: binary id;

  /// Create the file using a pre-existing file specified by content SHA-1.
  3: binary content_sha1;

  /// Create the file using a pre-existing file specified by content SHA-256.
  4: binary content_sha256;

  /// Create the file using a pre-existing file specified by content Git SHA-1
  /// This is the hash you get from `git hash-object -t blob ${OBJECT}`
  5: binary content_gitsha1;
}

struct RepoCreateCommitParamsFileCopyInfo {
  /// Path the file was copied from.
  1: string path;

  /// Index (in the list of the commit's parents) of the parent it was
  /// copied from.
  2: i32 parent_index;
}

@hack.MigrationBlockingLegacyJSONSerialization
union RepoCreateCommitParamsGitLfs {
  // File should be served as full text (bool val is ignored)
  1: bool full_content;
  // A canonical LFS pointer should be generated (bool val is ignored)
  2: bool lfs_pointer;
  // A non-canonical LFS pointer is provided
  3: RepoCreateCommitParamsFileContent non_canonical_lfs_pointer;
}

struct RepoCreateCommitParamsFileChanged {
  /// The new type of the file.
  1: RepoCreateCommitParamsFileType type;

  /// The new content of the file.
  2: RepoCreateCommitParamsFileContent content;

  /// The file was copied from another file.
  3: optional RepoCreateCommitParamsFileCopyInfo copy_info;

  /// Controls Git LFS representation of change in Git clones of this repo.
  /// (omitting this field lets server make the decision)
  4: optional RepoCreateCommitParamsGitLfs git_lfs;
}

struct RepoCreateCommitParamsFileDeleted {}

@hack.MigrationBlockingLegacyJSONSerialization
union RepoCreateCommitParamsChange {
  /// The file was created or changed.
  1: RepoCreateCommitParamsFileChanged changed;

  /// The file was deleted.
  2: RepoCreateCommitParamsFileDeleted deleted;
}

struct RepoCreateCommitParamsCommitInfo {
  /// The commit message.
  1: string message;

  /// The date the commit was authored. If omitted, the server will use the
  /// current time in its default timezone.
  2: optional DateTime date;

  /// The author of the commit.
  3: string author;

  /// Extra metadata about the commit.
  4: map<string, binary> extra;

  /// The identity that committed this commit, as opposed to wrote it
  5: optional string committer;

  /// The date the commit was committed.
  6: optional DateTime committer_date;

  /// Extra git headers associated with the commit if the commit is a
  /// mirrored version from a git repo.
  7: optional map<small_binary, binary_bytes> git_extra_headers;
}

struct RepoCreateCommitParams {
  /// The info for the new commit.
  1: RepoCreateCommitParamsCommitInfo info;

  /// The parents of the commit.
  2: list<CommitId> parents;

  /// A mapping from path to the change that is made at that path.
  ///
  /// Merge commits require changes to resolve all conflicts in the merge.
  /// When building a merge commit, the following rules apply:
  /// 1. All files that are present in at least one parent are in the pre-changes merge
  /// 2. Paths which differ between the parents they are present in are conflicted
  ///    and need a change to resolve the conflict
  /// 3. Where a path is a file in some parents, and a tree in others, resolving
  ///    the conflict with a "deleted" merely removes the file, leaving the trees
  ///    as part of the pre-changes merge. Resolving it with a "changed" recursively
  ///    deletes the trees.
  3: map<string, RepoCreateCommitParamsChange> changes;

  /// Commit identity schemes to return.
  4: set<CommitIdentityScheme> identity_schemes;

  /// Service identity to use for this commit creation.
  5: optional string service_identity;
}

struct RepoCreateStackParamsCommit {
  /// The info for the new commit.
  1: RepoCreateCommitParamsCommitInfo info;

  /// A mapping from path to the change that is made at that path.
  3: map<string, RepoCreateCommitParamsChange> changes;
}

struct RepoCreateStackParams {
  /// The stack of commits to create, in topological order, starting with the first commit.
  1: list<RepoCreateStackParamsCommit> commits;

  /// The parents of the first commit in the stack.
  2: list<CommitId> parents;

  /// Optional set of derived data types to derive for the newly created
  /// commits in preparation of future operations.  Derivation for the parents
  /// must already be complete.
  3: optional set<DerivedDataType> prepare_derived_data_types;

  /// Commit identity schemes to return.
  4: set<CommitIdentityScheme> identity_schemes;

  /// Service identity to use for this stack creation.
  5: optional string service_identity;
}

struct RepoCreateBookmarkParams {
  /// The name of the bookmark to create.
  1: string bookmark;

  /// The target commit for the bookmark.
  2: CommitId target;

  /// The pushvars to use when creating the bookmark.
  4: optional map<string, binary> pushvars;

  /// Service identity to use for this bookmark creation.
  3: optional string service_identity;
}

struct RepoMoveBookmarkParams {
  /// The name of the bookmark to move.
  1: string bookmark;

  /// The new target commit for the bookmark.
  2: CommitId target;

  /// The old bookmark target.  If provided, only move the bookmark if it
  /// points at this commit.
  5: optional CommitId old_target;

  /// Whether non-fast-forward moves are allowed (a.k.a. force move).
  ///
  /// By default, non-fast-forward moves are prevented.  Set this to `true` if
  /// you wish to allow a non-fast-forward move for the bookmark.
  ///
  /// Note: some bookmarks may be prevented from all non-fast-forward moves in
  /// the repository configuration.  This flag will *not* override that
  /// configuration.
  3: bool allow_non_fast_forward_move;

  /// The pushvars to use when moving the bookmark.
  6: optional map<string, binary> pushvars;

  /// Service identity to use for this bookmark move.
  4: optional string service_identity;
}

struct RepoDeleteBookmarkParams {
  /// The name of the bookmark to move.
  1: string bookmark;

  /// The old bookmark target.  If provided, only delete the bookmark if it
  /// points at this commit.
  2: optional CommitId old_target;

  /// The pushvars to use when deleting the bookmark.
  4: optional map<string, binary> pushvars;

  /// Service identity to use for this bookmark move.
  3: optional string service_identity;
}

enum CrossRepoPushSource {
  NATIVE_TO_THIS_REPO = 0,
  PUSH_REDIRECTED = 1,
}

enum BookmarkKindRestrictions {
  ANY_KIND = 0,
  ONLY_SCRATCH = 1,
  ONLY_PUBLISHING = 2,
}

struct RepoLandStackParams {
  /// The name of the bookmark to land to.
  1: string bookmark;

  /// The head commit of the stack that is to be landed.
  2: CommitId head;

  /// The parent of the bottom of the stack that is to be landed.  This must
  /// match the merge base of the head commit with respect to the current
  /// bookmark location.
  3: CommitId base;

  /// The set of commit identity schemes to return in the response.
  4: set<CommitIdentityScheme> identity_schemes;

  /// The commit identity schemes to use for the old commit ID of the
  /// pushrebased commits in the response.  This can be used to prevent
  /// derivation of alternative commit formats for the old commits where the
  /// caller does not care about them.  If not specified, then
  /// identity_schemes is used instead.
  5: optional set<CommitIdentityScheme> old_identity_schemes;

  /// The pushvars to use when landing the stack.
  7: optional map<string, binary> pushvars;

  /// Service identity to use for the bookmark move.
  6: optional string service_identity;

  // 101: deleted

  /// What kind of bookmark can be pushed
  9: BookmarkKindRestrictions bookmark_restrictions = BookmarkKindRestrictions.ANY_KIND;
}

struct RepoPrepareCommitsParams {
  /// The list of commits for which data must be derived
  1: list<CommitId> commits;
  /// The type of data that we want to derive
  2: DerivedDataType derived_data_type;
}

struct RepoUploadFileContentParams {
  /// Content of the new file.
  1: binary data;

  /// The expected content sha1 of the file.
  ///
  /// If provided, it will be an error if the content does not match this hash.
  2: optional binary expected_content_sha1;

  /// The expected content sha256 of the file.
  ///
  /// If provided, it will be an error if the content does not match this hash.
  3: optional binary expected_content_sha256;

  /// The expected content seeded-blake3 of the file.
  ///
  /// If provided, it will be an error if the content does not match this hash.
  4: optional binary expected_content_seeded_blake3;

  /// The identity of the service making the upload file content request.
  5: optional string service_identity;
}

/// Optional metadata for the commit that will be created
struct RepoUpdateSubmoduleExpansionCommitInfo {
  /// The commit message.
  1: optional string message;

  /// The author of the commit.
  2: optional string author;

  /// The date the commit was authored. If omitted, the server will use the
  /// current time in its default timezone.
  3: optional DateTime author_date;
}

/// Params for repo_update_submodule_expansion method
struct RepoUpdateSubmoduleExpansionParams {
  /// Large repo containing the expansion being updated
  1: RepoSpecifier large_repo;
  /// Large repo commit that will be the base commit for the generated commit
  /// updating the submodule expansion.
  2: CommitId base_commit_id;
  /// Path of the submodule expansion in the large repo
  3: Path submodule_expansion_path;
  /// New submodule commit to expand.
  /// NOTE: if this is set to null, the submodule expansion will be DELETED!
  /// This means deleting the submodule from the Git repo when backsyncing the
  /// commit.
  4: optional CommitId new_submodule_commit_or_delete;
  /// Commit identity schemes to return.
  5: set<CommitIdentityScheme> identity_schemes;
  /// Optional metadata for the commit that will be generated
  6: optional RepoUpdateSubmoduleExpansionCommitInfo commit_info;
}

struct CommitLookupParams {
  /// Commit identity schemes to return.
  1: set<CommitIdentityScheme> identity_schemes;
}

struct RepoMultipleCommitLookupParams {
  /// List of commit IDs to return.
  1: list<CommitId> commit_ids;

  /// Commit identity schemes to return for each changeset.
  2: set<CommitIdentityScheme> identity_schemes;
}

struct CommitLookupPushrebaseHistoryParams {}

struct CommitInfoParams {
  /// Commit identity schemes to return.
  1: set<CommitIdentityScheme> identity_schemes;
}

struct CommitGenerationParams {}

/// Parameters for the `commit_is_ancestor_of` method.
///
/// This method takes a commit specifier (the target commit), and checks
/// whether it is an ancestor of some other commit in the same repository.
struct CommitIsAncestorOfParams {
  /// Potentially descendant commit id to check whether or not the target
  /// commit is an ancestor of.
  1: CommitId descendant_commit_id;
}

struct CommitCommonBaseWithParams {
  1: CommitId other_commit_id;
  2: set<CommitIdentityScheme> identity_schemes;
}

const i64 COMMIT_COMPARE_ORDERED_MAX_LIMIT = 10000;

struct CommitCompareOrderedParams {
  // Restrict the set of paths to those after this path.  Set this
  // to continue diffing after a previous ordered request reached
  // its limit.
  1: optional Path after_path;
  // Limit the number of returned paths to this many.
  2: i64 limit;
}

struct CommitCompareParams {
  /// Commit to compare with. By default it's the commit's first parent.
  1: optional CommitId other_commit_id;
  /// Shows copies as just file adds, and renames as adds and dels.
  2: bool skip_copies_renames = false;
  /// Commit identity schemes to return.
  3: set<CommitIdentityScheme> identity_schemes;
  /// Restrict the comparison to the given paths and their descendants
  4: optional list<Path> paths;
  /// What to compare (default is FILES)
  5: set<CommitCompareItem> compare_items;
  /// If present, perform the compare in path order with these parameters.
  6: optional CommitCompareOrderedParams ordered_params;
  /// Whether to find parents via the commit ancestry, or via mutable copy
  /// information. If not supplied, a default will be chosen for you
  7: optional bool follow_mutable_file_history;
  /// When performing a comparison against a parent, follow subtree
  /// copy sources and show only the differences against those sources.
  8: optional bool compare_with_subtree_copy_sources;
}

struct CommitFileDiffsParamsSubtreeSource {
  1: CommitId commit_id;
  2: Path path;
}

struct CommitFileDiffsParamsPathPair {
  /// Missing base path shows file as removed.
  1: optional Path base_path;
  /// Missing other path shows file as added.
  2: optional Path other_path;
  /// Whether to render the diff as file move or copy
  /// (this method doesn't compute copy information)
  3: CopyInfo copy_info;
  /// If this option is set than instead of returning a real diff a placeholder
  /// diff like  `Binary files ... differs` is returned. This option might be
  /// useful to display diff for very large files (i.e. files that are above
  /// COMMIT_FILE_DIFFS_SIZE_LIMIT).
  4: optional bool generate_placeholder_diff;
  /// If the file being diffed was copied by a subtree copy, this value should
  /// be provided with the original subtree copy source.
  5: optional CommitFileDiffsParamsSubtreeSource subtree_source;
}

const i64 COMMIT_FILE_DIFFS_SIZE_LIMIT = 0x4000000; /// 64MiB
const i64 COMMIT_FILE_DIFFS_PATH_COUNT_LIMIT = 1000;

struct CommitFileDiffsParams {
  /// The commit to diff against.
  1: optional CommitId other_commit_id;
  /// List of paths to diffs: in a single request
  ///  * at most COMMIT_FILE_DIFFS_PATH_COUNT_LIMIT paths can be requested
  ///  * the total size of diffed files must be less than COMMIT_FILE_DIFFS_SIZE_LIMIT
  ///    unless `generate_placeholder_diff` is set in
  ///    CommitFileDiffsParamsPathPair
  2: list<CommitFileDiffsParamsPathPair> paths;
  3: DiffFormat format;
  /// Number of lines of unified context around differences (default: 3)
  4: i64 context = 3;
  /// Limit the size of the returned diff.  The meaning of this value depends on the
  /// diff format.  For raw diffs, it is the total size in bytes of the returned diffs.
  /// For phabricator diff metadata, it is the number of entries.
  5: optional i64 diff_size_limit;
}

const i64 COMMIT_FIND_FILES_MAX_LIMIT = 100000;

struct CommitFindFilesParams {
  /// Limit to the number of tree entries listed. If the request returns
  /// the limit, a subsequent call with 'after' set to the last path in the
  /// response is necessary to find the remaining files.
  1: i64 limit;

  /// Start listing files after this path, to be used for paging.  If
  /// specified, files will be listed ordered (use the empty string to
  /// start finding from the beginning).
  2: optional string after;

  /// Return entries where the entry's basename is in this array. Note that if
  /// basename_suffixes is provided, then entries are returned if an entry's
  /// basename is in basenames or a suffix of the entry's basename is in
  /// basename_suffixes. This means that if basename_suffixes is provided,
  /// returned entries basenames' may not be in this array.
  3: optional list<string> basenames;

  /// Return entries that have these path prefixes.
  4: optional list<string> prefixes;

  /// Return entries where a suffix of the entry's basename is in this array, if
  /// this array is provided.
  /// For example, if basename_suffixes is ['foo'], the basenames 'bar.foo',
  /// 'foo', and '.foo' will all match. However, 'bar' would not match.
  /// If the array is empty, nothing will match; however, basenames that are in
  /// the array basenames will match.
  5: optional list<string> basename_suffixes;
}

/// Parameters for the `commit_history` method.
///
/// By default, this will include all commits that are ancestors of
/// the target commit (including the commit itself).  This can be filtered
/// in a number of ways:
///
/// * `after_timestamp` will exclude any ancestors that are older than
///   this timestamp.  Traversal will stop at the first ancestor on each branch
///   that is too old, so if commits have been made out of order there may be
///   some ancestors with newer timestamps that are not returned.
///
/// * `before_timestamp` will exclude any ancestors that are newer than
///   this timestamp.
///
/// * `descendants_of` will restrict traversal to only those commits which
///   are **between** the target commit and this commit.  This means any
///   branches that are merged in after the `descendants_of` commit will
///   be excluded.  Use this when you want to find all the commits that
///   are between two commits.
///
/// * `exclude_changeset_and_ancestors` will prune traversal at the given
///   commit and any of its ancestors.  Unlike `descendants_of`, other
///   merged in branches will still be included, which may be a large
///   number of commits.
///
/// These options can be combined.  In particular, since `descendants_of`
/// is an inclusive range of commits, and `exclude_changeset_and_ancestors`
/// excludes the target commits, a half-open range of commits
/// `(ancestor, descendant]` can be obtained by setting both of these to
/// the ancestor commit.
struct CommitHistoryParams {
  /// Return history in the given format.
  1: HistoryFormat format;
  /// Number of commits to return in the history.
  2: i32 limit;
  /// Number of commits to skip before listing the history.
  3: i32 skip;
  /// Show commits created only before the given timestamp.
  /// Timestamp must be greater than 0.
  4: optional i64 before_timestamp;
  /// Show commits created only after the given timestamp.
  /// Timestamp must be greater than 0.
  5: optional i64 after_timestamp;
  /// Commit identity schemes to return in the commit information.
  6: set<CommitIdentityScheme> identity_schemes;
  /// Include only commits that are descendants of the given commit (including
  /// the commit itself)
  7: optional CommitId descendants_of;
  /// Exclude commit and all of its ancestor from results.
  8: optional CommitId exclude_changeset_and_ancestors;
}

/// Parameters for the `commit_linear_history` method.
///
/// By default, this will include all commits that are linear ancestors of
/// the target commit. Linear ancestors are commits that can be reached by
/// following by following the first parent of the commit (including the
/// commit itself). This can be filtered in a number of ways:
///
/// * `descendants_of` will restrict traversal to only those commits which
///   are linear descendants of the given commit, i.e. commits that can
///   reach the given commit by following the first parent.
///
/// * `exclude_changeset_and_ancestors` will prune traversal at the given
///   commit and any of its linear ancestors.
///
/// These options can be combined.  In particular, since `descendants_of`
/// is an inclusive range of commits, and `exclude_changeset_and_ancestors`
/// excludes the target commits, a half-open range of commits
/// `(ancestor, descendant]` can be obtained by setting both of these to
/// the ancestor commit.
struct CommitLinearHistoryParams {
  /// Return history in the given format.
  1: HistoryFormat format;
  /// Number of commits to return in the history.
  2: i32 limit;
  /// Number of commits to skip before listing the history.
  3: i64 skip;
  /// Commit identity schemes to return in the commit information.
  6: set<CommitIdentityScheme> identity_schemes;
  /// Include only commits that are linear descendants of the given commit
  /// (including the commit itself)
  7: optional CommitId descendants_of;
  /// Exclude commit and all of its linear ancestor from results.
  8: optional CommitId exclude_changeset_and_ancestors;
}

const i64 COMMIT_LIST_DESCENDANT_BOOKMARKS_MAX_LIMIT = 10000;

struct CommitListDescendantBookmarksParams {
  /// If true, include scratch bookmarks. To list scratch bookmarks, you
  /// must provide a non-empty prefix and a limit.
  1: bool include_scratch;

  /// Prefix to match when listing bookmarks.
  2: string bookmark_prefix;

  /// Limit to the number of bookmarks that may match.
  3: i64 limit;

  /// Return bookmarks after this name, to be used for paging.
  4: optional string after;

  /// Commit identity schemes to return.
  5: set<CommitIdentityScheme> identity_schemes;
}

struct CommitRunHooksParams {
  /// Run the same hooks as when landing to bookmark
  1: string bookmark;
  /// Pushvars used on the push.
  2: optional map<string, binary> pushvars;
}

struct CommitSubtreeChangesParams {
  /// Commit identity schemes to return.
  1: set<CommitIdentityScheme> identity_schemes;
}

struct CommitHgMutationHistoryParams {
  /// The format of the mutation history to return.
  1: MutationHistoryFormat format;
}

struct CommitPathExistsParams {}

struct CommitPathInfoParams {}

struct CommitMultiplePathInfoParams {
  /// List of paths to query.
  ///
  /// Note: paths that do not exist will be omitted from the response.
  1: list<Path> paths;
}

const i64 TREE_LIST_MAX_LIMIT = 10000;

struct CommitPathBlameParams {
  /// Which format to use in the response.
  1: BlameFormat format;

  /// Commit identity schemes to return.
  3: set<CommitIdentityScheme> identity_schemes;

  /// Options to customize the blame format.  The interpretation of these is
  /// up to the blame format.
  ///
  /// If not specified, defaults to {INCLUDE_CONTENT}.
  4: optional set<BlameFormatOption> format_options;

  /// Use mutable copy information to identify ancestry, instead of
  /// using commit parents to identify ancestry
  5: optional bool follow_mutable_file_history;
}

/// Parameters for the `commit_path_history` method.
///
/// By default, this will include all commits that are ancestors of
/// the target commit (including the commit itself) that modify the target
/// path.  This can be filtered in the same ways as the `commit_path` method.
/// See `CommitHistoryParams` for more details.
struct CommitPathHistoryParams {
  /// Return history in the given format.
  1: HistoryFormat format;
  /// Number of commits to return in the history.
  2: i32 limit;
  /// Number of commits to skip before listing the history.
  3: i32 skip;
  /// Show commits created only before the given timestamp.
  /// Timestamp must be greater than 0.
  4: optional i64 before_timestamp;
  /// Show commits created only after the given timestamp.
  /// Timestamp must be greater than 0.
  5: optional i64 after_timestamp;
  /// Commit identity schemes to return in the commit information.
  6: set<CommitIdentityScheme> identity_schemes;
  /// Tracks history of a path even if it was deleted and then reintroduced
  /// This is more expensive and generally discouraged to use.
  7: bool follow_history_across_deletions;
  /// Include only commits that are descendants of the given commit (including
  /// the commit itself)
  8: optional CommitId descendants_of;
  /// Exclude commit and all of its ancestor from results.
  9: optional CommitId exclude_changeset_and_ancestors;
  /// Use mutable copy information to identify ancestry, instead of
  /// using commit parents to identify ancestry
  10: optional bool follow_mutable_file_history;
}

struct CommitPathLastChangedParams {
  /// Commit identity schemes to return.
  1: set<CommitIdentityScheme> identity_schemes;
}

struct CommitMultiplePathLastChangedParams {
  /// List of paths to query.
  ///
  /// Note: paths that have never existed will be omitted from the response.
  1: list<Path> paths;

  /// Commit identity schemes to return.
  2: set<CommitIdentityScheme> identity_schemes;
}

struct CommitSparseProfileDeltaToken {
  1: i64 id;
}

struct CommitSparseProfileDeltaParamsV2 {
  1: CommitSpecifier commit;
  /// Revision on which inspect sparse profiles
  2: CommitId other_id;
  /// list of sparse profiles for which calculate size change
  3: SparseProfiles profiles;
}

struct CommitSparseProfileSizeToken {
  /// A target this token relates to
  1: SparseProfiles target;
  /// An actual token payload
  2: i64 id;
}

struct CommitSparseProfileSizeParamsV2 {
  1: CommitSpecifier commit;
  /// list of sparse profiles for which calculate total size
  2: SparseProfiles profiles;
}

struct TreeExistsParams {}

struct TreeListParams {
  /// Start listing at this offset in the tree.
  1: i64 offset;

  /// Limit to the number of tree entries listed.
  2: i64 limit;
}

struct FileExistsParams {}

struct FileInfoParams {}

const i64 FILE_CONTENT_CHUNK_SIZE_LIMIT = 0x1000000; /// 16MiB

/// Recommended chunk size for file_content_chunk requests.  This is just a
/// suggestion - the client may use any chunk size up to the limit and the
/// server will re-chunk as necessary.
const i64 FILE_CONTENT_CHUNK_RECOMMENDED_SIZE = 0x3FFC00; /// 4MiB - 1KiB

struct FileContentChunkParams {
  /// The offset within the file to fetch.
  1: i64 offset;

  /// The requested chunk size. If the requested size from this offset goes
  /// past the end of the file, then only the bytes up to the end of the
  /// file are returned.
  2: i64 size;
}

struct FileDiffParams {
  /// The ID of the other file, obtained from a previous response.
  1: binary other_file_id;

  /// Diff format to return,
  3: DiffFormat format;

  /// Number of lines of unified context around differences (default: 3)
  4: i64 context = 3;
}

@hack.MigrationBlockingLegacyJSONSerialization
union CandidateSelectionHint {
  /// Select an ancestor of a given bookmark
  1: string bookmark_ancestor;
  /// Select a descendant of a given bookmark
  2: string bookmark_descendant;
  /// Select an ancestor of a given commit
  3: CommitId commit_ancestor;
  /// Select a descendant of a given commit
  4: CommitId commit_descendant;
  /// Select a given commit from a list of candidates
  5: CommitId exact;
}

struct CommitLookupXRepoParams {
  /// The other repo to look in
  1: RepoSpecifier other_repo;
  /// Commit identity schemes to return.
  2: set<CommitIdentityScheme> identity_schemes;
  /// Candidate selection hint for resolving plural
  /// mapping situations
  3: optional CandidateSelectionHint candidate_selection_hint;
  /// Do not sync the requests commit on-demand. Returns quicker with result or not-existing mapped
  /// commit if the commit wasn't synced yet.
  4: bool no_ondemand_sync;
  /// Return result only if there's exact match for the requested commit - rather than commit with
  /// equivalent working copy (which happens in case the source commit rewrites to nothing in target
  /// repo).
  5: bool exact;
}

struct CustomAclParams {
  1: string hipster_group;
}

enum RepoSizeBucket {
  /// <100MB
  EXTRA_SMALL = 0,
  /// <1GB
  SMALL = 1,
  /// <10GB
  MEDIUM = 2,
  /// <100GB
  LARGE = 3,
  /// >100GB
  EXTRA_LARGE = 4,
}

enum RepoScmType {
  // Only git repos can be created via this API now.
  GIT = 0,
}

struct RepoCreationRequest {
  1: string repo_name;
  /// What kind of repo should it be? (sl or git)
  2: RepoScmType scm_type;
  /// Oncall Owning he repo
  3: string oncall_name;
  /// Hipster group owning a custom ACL (if custom ACL is required)
  4: optional CustomAclParams custom_acl;
  /// Size bucket (allows for provisioning the right amount of resources for the new repo)
  5: RepoSizeBucket size_bucket;
}

struct CreateReposParams {
  /// Lists of repos to create
  1: list<RepoCreationRequest> repos;
  /// Dry run:
  2: bool dry_run;
}

struct CreateReposToken {}

/// Synchronization target
@rust.Ord
struct MegarepoTarget {
  /// Mononoke repository id, where the target is located
  /// At least one of repo/repo_id must be set in queries both are set in responses
  1: optional i64 repo_id;
  /// Bookmark, which this target represents
  2: string bookmark;
  /// Repo
  /// At least one of repo/repo_id must be set in queries both are set in responses
  3: optional RepoSpecifier repo;
}

/// A single version of synchronization config for a target,
/// bundling together all of the corresponding sources
struct MegarepoSyncTargetConfig {
  // A target to which this config can apply
  1: MegarepoTarget target;
  // All of the sources to sync from
  2: list<megarepo_configs.Source> sources;
  // The version of this config
  3: megarepo_configs.SyncConfigVersion version;
}

/// Polling tokens for async megarepo methods
struct MegarepoChangeConfigToken {
  // 1: deprecated
  /// An actual token payload
  2: i64 id;
}

struct MegarepoSyncChangesetToken {
  // 1: deprecated
  /// An actual token payload
  2: i64 id;
}
struct MegarepoRemergeSourceToken {
  // 1: deprecated
  /// An actual token payload
  2: i64 id;
}
struct MegarepoAddTargetToken {
  // 1: deprecated
  /// An actual token payload
  2: i64 id;
}

struct MegarepoAddBranchingTargetToken {
  // 1: deprecated
  /// An actual token payload
  2: i64 id;
}

/// Params for the megarepo_add_sync_target_config method
struct MegarepoAddConfigParams {
  /// New config to be added to the config library
  /// the config *must* refer to an existing target
  /// Config's version *must* be different from
  /// any previously used config version
  1: MegarepoSyncTargetConfig new_config;
}

/// Params for the megarepo_read_target_config method
struct MegarepoReadConfigParams {
  1: MegarepoTarget target;
  2: CommitId commit;
}

/// Params for megarepo_add_sync_target method
struct MegarepoAddTargetParams {
  /// Initial config to be used on the new target
  /// The config *must* refer to the not-yet-existing target
  /// which will be recorded as new target
  /// Config's version must be different from
  /// any previously used config version
  1: MegarepoSyncTargetConfig config_with_new_target;
  /// Initial changesets to merge for each of the
  /// sources in the `config_with_new_target`.
  /// While each source provides a revision to
  /// be followed in the future, at the moment of
  /// the initial target creation, it may be needed
  /// to merge an ancestor of said revision. That is
  /// why this field exists. Each source's changeset
  /// MUST be an ancestor of the source revision if the
  /// source revision is a bookmark, and it MUST be equal
  /// to the source revision if it is a changeset itself
  /// Each source name MUST be present in this map.
  2: map<string, megarepo_configs.ChangesetId> changesets_to_merge;
  /// A message to be used in the commit description
  /// If not provided, service will generate commit description
  3: optional string message;
}

/// Params for megarepo_add_sync_target method
struct MegarepoAddBranchingTargetParams {
  /// A new target to be created
  1: MegarepoTarget target;
  /// The specified commit will be used as parent of the first commit in the
  /// newly created target. The megarepo config used to create the branching
  /// point will be used as the base for the new target config.
  2: megarepo_configs.ChangesetId branching_point;
  /// The specified source target to use as the source of config for this
  /// new target. This call will verify that branching_point is a valid
  /// commit to use with that source target
  3: MegarepoTarget source_target;
}

/// Params for megarepo_change_target_config method
struct MegarepoChangeTargetConfigParams {
  /// A target for which to change the version
  1: MegarepoTarget target;
  /// New config version to be used for the target above
  /// This version *must* refer to the `target`
  2: megarepo_configs.SyncConfigVersion new_version;
  /// Current location of the `target`'s bookmark.
  /// This argument exists to prevent race conditions
  3: megarepo_configs.ChangesetId target_location;
  /// Initial changesets to merge for each of the
  /// sources in the `target`. Similar to `changesets_to_merge`
  /// in the `MegarepoAddTargetParams` struct, see docstring
  /// there
  4: map<string, megarepo_configs.ChangesetId> changesets_to_merge;
  /// A message to be used in the commit description
  /// If not provided, service will generate commit description
  5: optional string message;
}

/// Params for megarepo_sync_changeset method
struct MegarepoSyncChangesetParams {
  /// Source from which to sync the changeset
  1: string source_name;
  /// Target into which to sync the changeset
  2: MegarepoTarget target;
  /// This operation will sync all not-yet synced
  /// changesets up to and including `cs_id` from
  /// `source` into `target`
  3: megarepo_configs.ChangesetId cs_id;
  /// Current location of the `target`'s bookmark.
  /// This argument exists to prevent race conditions
  4: megarepo_configs.ChangesetId target_location;
}

/// Params for megarepo_re_merge_source method
struct MegarepoRemergeSourceParams {
  /// Source which needs remerging
  1: string source_name;
  /// Target into which to remerge the source
  2: MegarepoTarget target;
  /// Remerge source at `cs_id` and mark `cs_id`
  /// as the last synced changeset form this source
  /// Note: this does not do any ancestry checks
  /// with previous changesets synced form the same
  /// source
  3: megarepo_configs.ChangesetId cs_id;
  /// Current location of the `target`'s bookmark.
  /// This argument exists to prevent race conditions
  4: megarepo_configs.ChangesetId target_location;
  /// A message to be used in the commit description
  /// If not provided, service will generate commit description
  5: optional string message;
}

/// Params for repo_upload_non_blob_git_object method
struct RepoUploadNonBlobGitObjectParams {
  /// The raw bytes of the hash of the git object that is being uploaded.
  /// In git terminology, this is the git object_id in bytes
  1: binary git_hash;
  /// The raw content of the git object that is being uploaded.
  2: binary raw_content;
  /// The identity of the service making the upload git object request.
  3: optional string service_identity;
}

/// Params for upload_packfile_base_item method
struct RepoUploadPackfileBaseItemParams {
  /// The raw bytes of the hash of the git object that is being uploaded as packfile base item.
  /// In git terminology, this is the git object_id in bytes
  1: binary git_hash;
  /// The raw content of the git object that is being uploaded as packfile base item.
  2: binary raw_content;
  /// The identity of the service making the upload packfile base item request.
  3: optional string service_identity;
}

/// Params for create_git_tree method
struct CreateGitTreeParams {
  /// The raw bytes of the hash of the git tree object that is being stored
  /// as a bonsai changeset at Mononoke end
  1: binary git_tree_hash;
  /// The identity of the service making the create git tree request.
  2: optional string service_identity;
}

/// Params for create_git_tag method
struct CreateGitTagParams {
  /// The author of the tag, if it exists
  1: optional string author;
  /// The date of tag creation, if it exists
  2: optional DateTime author_date;
  /// The annotation or message of the tag
  3: string annotation;
  /// The pgp signature associated with the tag, if one exists
  4: optional binary pgp_signature;
  /// The changeset corresponding to the commit that was pointed at by the tag.
  5: binary target_changeset;
  /// The identity of the service making the create git tag request.
  6: optional string service_identity;
  /// The name of the tag for which the changeset is getting created
  7: string tag_name;
  /// The git SHA1 hash of the tag
  8: optional binary tag_hash;
  /// Flag indicating if the target of the tag is also a tag
  9: optional bool target_is_tag;
}

/// Method response structures

struct RepoResolveBookmarkResponse {
  /// Whether the bookmark exists.
  1: bool exists;

  /// The bookmarked commit's IDs in the requested schemes (if available).
  2: optional map<CommitIdentityScheme, CommitId> ids;
}

enum RepoResolveCommitPrefixResponseType {
  RESOLVED = 0,
  AMBIGUOUS = 1,
  NOT_FOUND = 2,
}

struct RepoResolveCommitPrefixResponse {
  1: RepoResolveCommitPrefixResponseType resolved_type;

  /// The resolve commit IDs in the requested schemes (if type == RESOLVED)
  2: optional map<CommitIdentityScheme, CommitId> ids;
}

struct RepoBookmarkInfoResponse {
  /// Bookmark info, null if doesn't exist.
  1: optional BookmarkInfo info;
}

struct RepoListBookmarksResponse {
  /// A map from bookmark name to the bookmarked commit's IDs in the
  /// requested schemes (if available).
  1: map<string, map<CommitIdentityScheme, CommitId>> bookmarks;

  /// If set, there are potentially more bookmarks.  Provide this
  /// bookmark name as the `after` parameter in a new request to
  /// continue finding them.
  2: optional string continue_after;
}

struct RepoStackInfoResponse {
  /// Draft commits in topological order.
  1: list<CommitInfo> draft_commits;

  /// Public roots in topological order.
  2: list<CommitInfo> public_parents;

  /// List of commits that weren't considered yet because the limit was
  /// reached.  To find more, call repo_stack_info again with this list as the
  /// list of heads.  Note that shared ancestry may result in duplicate
  /// commits in subsequent calls.
  3: list<map<CommitIdentityScheme, CommitId>> leftover_heads;
}

/// The response of the repo_stack_git_bundle_store method
struct RepoStackGitBundleStoreResponse {
  1: string everstore_handle;
}

struct RepoCreateCommitResponse {
  /// The IDs of the created commit.
  1: map<CommitIdentityScheme, CommitId> ids;
}

struct RepoCreateStackResponse {
  /// The IDs of the created commits in the stack.
  1: list<map<CommitIdentityScheme, CommitId>> commit_ids;
}

struct RepoCreateBookmarkResponse {}

struct RepoMoveBookmarkResponse {}

struct RepoDeleteBookmarkResponse {}

struct RepoLandStackResponse {
  1: PushrebaseOutcome pushrebase_outcome;
}

struct RepoPrepareCommitsResponse {}

struct RepoUploadFileContentResponse {
  /// The id of the uploaded file, which can be used in subsequent
  /// repo_create_commit requests.
  1: binary id;
}

struct RepoUpdateSubmoduleExpansionResponse {
  /// IDs of the commit updating the submodule expansion in the provided
  /// repo in all the requested schemes.
  1: map<CommitIdentityScheme, CommitId> ids;
}

@hack.MigrationBlockingLegacyJSONSerialization
union RepoUpdateSubmoduleExpansionResult {
  1: RepoUpdateSubmoduleExpansionResponse success;
  2: MegarepoAsynchronousRequestError error;
}

struct CommitCompareResponse {
  /// List of the files that are different between commits with their metadata
  /// Can be used for subsequent `commit_path_diff` calls for file-level diffs.
  /// Only populated if FILES was specified.
  1: list<CommitCompareFile> diff_files;
  /// Commit that was used for comparison
  2: optional map<CommitIdentityScheme, CommitId> other_commit_ids;
  /// List of the dirs that are different between commits with their metadata
  /// Only populated if TREES was specified.
  3: list<CommitCompareTree> diff_trees;
  /// Only set if commit compare was ordered, and the limit was reached.
  /// This is the last path that was produced, suitable for passing into
  /// the `after_path` parameter of a subsequent ordered request.
  4: optional Path last_path;
}

struct CommitFileDiffsResponseElement {
  1: optional Path base_path;
  2: optional Path other_path;
  3: Diff diff;
  4: optional CommitId other_commit_id;
}

struct CommitFileDiffsStoppedAtPair {
  1: optional Path base_path;
  2: optional Path other_path;
}

struct CommitFileDiffsResponse {
  1: list<CommitFileDiffsResponseElement> path_diffs;
  /// The first pair for which a diff was not returned. Start next request from this pair if you want to resume.
  2: optional CommitFileDiffsStoppedAtPair stopped_at_pair;
}

struct CommitLookupResponse {
  /// Whether the commit exists.
  1: bool exists;

  /// The commit's IDs in the requested schemes (if available).
  2: optional map<CommitIdentityScheme, CommitId> ids;
}

struct CommitLookupEntry {
  1: CommitId commit_id;
  2: CommitLookupResponse commit_lookup_response;
}

struct RepoMultipleCommitLookupResponse {
  1: list<CommitLookupEntry> responses;
}

struct CommitLookupPushrebaseHistoryResponse {
  1: list<CommitSpecifier> history;
  /// Always equals to the last element of history
  2: CommitSpecifier origin;
}

struct CommitFindFilesResponse {
  /// The files that match.
  1: list<string> files;
}

struct CommitFindFilesStreamResponse {}

struct CommitFindFilesStreamItem {
  /// The files that match.
  1: list<string> files;
}

struct CommitHistoryResponse {
  1: History history;
}

struct CommitLinearHistoryResponse {
  1: History history;
}

struct CommitListDescendantBookmarksResponse {
  /// The map of bookmarks that are descendants of this bookmark and
  /// the commit they refer to.
  1: map<string, map<CommitIdentityScheme, CommitId>> bookmarks;

  /// If set, there are potentially more bookmarks.  Provide this
  /// bookmark name as the `after` parameter in a new request to
  /// continue finding them.
  2: optional string continue_after;
}

struct HookOutcomeAccepted {}

struct HookOutcomeRejected {
  /// A short description for summarizing this failure with similar failures
  1: string description;
  /// A full explanation of what went wrong, suitable for presenting to the user (should include guidance for fixing this failure, where possible)
  2: string long_description;
}

@hack.MigrationBlockingLegacyJSONSerialization
union HookOutcome {
  1: HookOutcomeAccepted accepted;
  3: list<HookOutcomeRejected> rejections;
}

struct CommitRunHooksResponse {
  1: map<string, HookOutcome> outcomes;
}

struct SubtreeCopy {
  1: Path source_path;
  2: map<CommitIdentityScheme, CommitId> source_commit_ids;
}

struct SubtreeMerge {
  1: Path source_path;
  2: map<CommitIdentityScheme, CommitId> source_commit_ids;
}

struct SubtreeImport {
  1: Path source_path;
  2: string source_commit_id;
  3: string source_url;
}

@hack.MigrationBlockingLegacyJSONSerialization
union SubtreeChange {
  1: SubtreeCopy subtree_copy;

  2: SubtreeMerge subtree_merge;

  3: SubtreeImport subtree_import;
}

struct CommitSubtreeChangesResponse {
  /// Map from destination path to the change that was performed on it.
  1: map<Path, SubtreeChange> subtree_changes;
}

struct CommitHgMutationHistoryResponse {
  1: HgMutationHistory hg_mutation_history;
}

@hack.MigrationBlockingLegacyJSONSerialization
union HgMutationHistory {
  1: list<CommitId> commit_ids;
}

struct CommitPathExistsResponse {
  /// Whether anything exists at this path.
  1: bool exists;

  /// Whether a file exists at this path.
  2: bool file_exists;

  /// Whether a tree (directory) exists at this path.
  3: bool tree_exists;
}

struct CommitPathInfoResponse {
  /// Whether an item exists at this path.
  1: bool exists;

  /// The type of the item at this path (file, link, exec, directory or submodule).
  2: optional EntryType type;

  /// The info for the item.
  3: optional EntryInfo info;
}

struct CommitMultiplePathInfoResponse {
  /// Path info for the requested paths.
  ///
  /// Note: requested paths that do not exist are omitted from the map.
  1: map<Path, CommitPathInfoResponse> path_info;
}

struct CommitPathBlameResponse {
  1: Blame blame;
}

struct CommitPathHistoryResponse {
  1: History history;
}

struct CommitPathLastChange {
  /// Whether anything exists at this path in this commit.
  1: bool exists;

  /// The commit that last changed this path.
  ///
  /// If something exists at this path, this contains the commit in which it
  /// was last changed.
  ///
  /// If nothing exists at this path, but something used to and has been
  /// deleted, this is the commit it was deleted in.
  2: map<CommitIdentityScheme, CommitId> last_changed_commit;
}

struct CommitPathLastChangedResponse {
  /// The last change for this path.  Not present if the path never existed.
  1: optional CommitPathLastChange last_change;
}

struct CommitMultiplePathLastChangedResponse {
  /// Last change for the requested paths.
  ///
  /// Requested paths that have never existed are omitted.
  1: map<Path, CommitPathLastChange> path_last_change;
}

struct CommitSparseProfileDeltaResponse {
  /// If any sparse profile changed, this contains change for each profile
  1: optional SparseProfileDeltaSizes changed_sparse_profiles;
}

@hack.MigrationBlockingLegacyJSONSerialization
union CommitSparseProfileDeltaPollResponse {
  1: PollPending poll_pending;
  2: CommitSparseProfileDeltaResponse response;
}

struct CommitSparseProfileSizeResponse {
  1: SparseProfileSizes profiles_size;
}

@hack.MigrationBlockingLegacyJSONSerialization
union CommitSparseProfileSizeResult {
  1: CommitSparseProfileSizeResponse success;
  2: AsyncRequestError error;
}

struct PollPending {}

@hack.MigrationBlockingLegacyJSONSerialization
union CommitSparseProfileSizePollResponse {
  1: PollPending poll_pending;
  2: CommitSparseProfileSizeResponse response;
}

struct TreeListResponse {
  /// The directory entries in this directory, at the offset requested,
  /// limited by the limit requested.
  1: list<TreeEntry> entries;

  /// The total number of entries in this directory. If this is greater
  /// than the requested limit, then more requests to get the rest of the
  /// list will be required.
  2: i64 count;
}

struct FileDiffResponse {
  /// The differences between the two files.
  1: Diff diff;
}

struct CreateReposResponse {
/// Indicates successful repo creation.
}

struct CreateReposPollResponse {
  /// Maybe a response to an underlying call, if it is ready
  1: optional CreateReposResponse result;
}

struct MegarepoAddConfigResponse {}

struct MegarepoReadConfigResponse {
  1: MegarepoSyncTargetConfig config;
}

struct MegarepoAddTargetResponse {
  /// A new position of the target bookmark
  /// after the "sync changeset" operation finished
  1: megarepo_configs.ChangesetId cs_id;
}

@hack.MigrationBlockingLegacyJSONSerialization
union MegarepoAddTargetResult {
  1: MegarepoAddTargetResponse success;
  2: MegarepoAsynchronousRequestError error;
}

struct MegarepoAddTargetPollResponse {
  /// Maybe a response to an underlying call, if it is ready
  1: optional MegarepoAddTargetResult result;
}

struct MegarepoAddBranchingTargetResponse {
  /// A new position of the target bookmark
  1: megarepo_configs.ChangesetId cs_id;
}

@hack.MigrationBlockingLegacyJSONSerialization
union MegarepoAddBranchingTargetResult {
  1: MegarepoAddBranchingTargetResponse success;
  2: MegarepoAsynchronousRequestError error;
}

struct MegarepoAddBranchingTargetPollResponse {
  /// Maybe a response to an underlying call, if it is ready
  1: optional MegarepoAddBranchingTargetResult result;
}

struct MegarepoChangeTargetConfigResponse {
  /// A new position of the target bookmark
  /// after the "change config" operation finished
  1: megarepo_configs.ChangesetId cs_id;
}

@hack.MigrationBlockingLegacyJSONSerialization
union MegarepoChangeTargetConfigResult {
  1: MegarepoChangeTargetConfigResponse success;
  2: MegarepoAsynchronousRequestError error;
}

struct MegarepoChangeTargetConfigPollResponse {
  /// Maybe a response to an underlying call, if it is ready
  1: optional MegarepoChangeTargetConfigResult result;
}

struct MegarepoSyncChangesetResponse {
  /// A new position of the target bookmark
  /// after the "sync changeset" operation finished
  1: megarepo_configs.ChangesetId cs_id;
}

@hack.MigrationBlockingLegacyJSONSerialization
union MegarepoSyncChangesetResult {
  1: MegarepoSyncChangesetResponse success;
  2: MegarepoAsynchronousRequestError error;
}

struct MegarepoSyncChangesetPollResponse {
  /// Maybe a response to an underlying call, if it is ready
  1: optional MegarepoSyncChangesetResult result;
}

struct MegarepoRemergeSourceResponse {
  /// A new position of the target bookmark
  /// after the "remerge source" operation finished
  1: megarepo_configs.ChangesetId cs_id;
}

@hack.MigrationBlockingLegacyJSONSerialization
union MegarepoRemergeSourceResult {
  1: MegarepoRemergeSourceResponse success;
  2: MegarepoAsynchronousRequestError error;
}

struct MegarepoRemergeSourcePollResponse {
  /// Maybe a response to an underlying call, if it is ready
  1: optional MegarepoRemergeSourceResult result;
}

struct RepoUploadNonBlobGitObjectResponse {}

struct RepoUploadPackfileBaseItemResponse {}

struct CreateGitTreeResponse {}

struct CreateGitTagResponse {
  /// The changeset ID of the changeset that was created for the git tag
  1: binary created_changeset_id;
}

/// Specifies a commit cloud workspace
@rust.Ord
struct WorkspaceSpecifier {
  /// The repository associated with the workspace.
  1: RepoSpecifier repo;
  /// Workspace name (user/<unixname>/<workspace_name>)
  2: string name;
}

/// Information about a commit cloud workspace
struct WorkspaceInfo {
  /// Workspace name and the repo it's associated with
  1: WorkspaceSpecifier specifier;
  /// Whether the workspace has been archived
  2: bool is_archived;
  /// Latest version number of the workspace
  3: i64 latest_version;
  /// Latest timestamp the workspace was updated
  4: i64 latest_timestamp;
}

/// Represents a remote bookmark in a commit cloud workspace.
struct WorkspaceRemoteBookmark {
  /// Prefix (usually 'remote').
  1: string remote;
  /// Bookmark name, e.g., 'master'.
  2: string name;
  /// Optional Mercurial commit ID the remote bookmark is associated with.
  3: optional string hg_id;
}

/// Represents a single commit within a workspace.
struct SmartlogNode {
  /// Mercurial commit ID.
  1: string hg_id;
  /// Whether the commit is public or draft.
  2: string phase;
  /// The author of the commit.
  3: string author;
  /// The date the commit was authored, as a Unix timestamp.
  4: i64 date;
  /// A message to be used in the commit description.
  5: string message;
  /// The parents of the commit.
  6: list<string> parents;
  /// Local bookmarks associated with the commit.
  7: list<string> bookmarks;
  /// Optional list of remote bookmarks associated with the commit.
  8: optional list<WorkspaceRemoteBookmark> remote_bookmarks;
}

/// Represents the Smartlog view of the contents of a commit cloud workspace.
struct SmartlogData {
  /// List of nodes that make up the Smartlog.
  1: list<SmartlogNode> nodes;
  /// Optional version number of the workspace being retrieved.
  2: optional i64 version;
  /// Optional timestamp of when the workspace version was updated.
  3: optional i64 timestamp;
}

enum CloudWorkspaceSmartlogFlags {
  /// Do not provide metadata about public commits in the response
  /// (by default both draft commits and their public roots are returned)
  SKIP_PUBLIC_COMMITS_METADATA = 1,
  /// return all remote bookmarks
  /// (by default only remote bookmarks that belong to draft commits (scratch bookmarks) or their public roots are returned)
  ADD_REMOTE_BOOKMARKS = 3,
  /// return all local bookmarks
  /// (by default only bookmarks that belong to draft commits or their public roots are returned)
  ADD_ALL_BOOKMARKS = 4,
}

struct CloudWorkspaceInfoParams {
  /// Workspace name and the repo it's associated with
  1: WorkspaceSpecifier workspace;
}

struct CloudWorkspaceInfoResponse {
  /// General info about the workspace, similar to `sl cloud status`
  1: WorkspaceInfo workspace_info;
}

struct CloudUserWorkspacesParams {
  /// Repo the workspace is associated with
  1: RepoSpecifier repo;
  /// User unixname whose workspace list is being queried
  2: string user;
}

struct CloudUserWorkspacesResponse {
  /// Workspaces associated with a certain user in a speific repo
  1: list<WorkspaceInfo> workspaces;
}

struct CloudWorkspaceSmartlogParams {
  /// Workspace name and the repo it's associated with
  1: WorkspaceSpecifier workspace;
  /// Options about what info to include in the response
  2: list<CloudWorkspaceSmartlogFlags> flags;
}

struct CloudWorkspaceSmartlogResponse {
  /// Smartlog view of a commit cloud workspace
  1: SmartlogData smartlog;
}

// Note that this method has no repo nor commit specifier–that is intentional, tests depend on that.
struct AsyncPingParams {
  /// The request payload, which will be echoed back into the response.
  1: string payload;
}

struct AsyncPingToken {
  1: i64 id;
}

struct AsyncPingResponse {
  1: string payload;
}

@hack.MigrationBlockingLegacyJSONSerialization
union AsyncPingPollResponse {
  1: PollPending poll_pending;
  2: AsyncPingResponse response;
}

/// Exceptions

enum RequestErrorKind {
  UNKNOWN = 0,
  INVALID_REQUEST = 1,
  REPO_NOT_FOUND = 2,
  COMMIT_NOT_FOUND = 3,
  FILE_NOT_FOUND = 4,
  TREE_NOT_FOUND = 5,
  INVALID_REQUEST_INPUT_TOO_BIG = 6,
  INVALID_REQUEST_TOO_MANY_PATHS = 7,
  PERMISSION_DENIED = 8,
  NOT_AVAILABLE = 9,
  NOT_IMPLEMENTED = 10,
  MERGE_CONFLICTS = 11,
  LARGE_REPO_NOT_FOUND = 12,
}

stateful client exception RequestError {
  1: RequestErrorKind kind;
  @thrift.ExceptionMessage
  2: string reason;
}

transient server exception InternalError {
  @thrift.ExceptionMessage
  1: string reason;
  2: optional string backtrace;
  3: list<string> source_chain;
}

transient server exception OverloadError {
  @thrift.ExceptionMessage
  1: string reason;
}

transient server exception PollError {
  @thrift.ExceptionMessage
  1: string reason;
}

struct RequestErrorStruct {
  1: RequestErrorKind kind;
  2: string reason;
}

struct InternalErrorStruct {
  1: string reason;
  2: optional string backtrace;
  3: list<string> source_chain;
}

@hack.MigrationBlockingLegacyJSONSerialization
union AsyncRequestError {
  1: RequestErrorStruct request_error;
  2: InternalErrorStruct internal_error;
}

@hack.MigrationBlockingLegacyJSONSerialization
union MegarepoAsynchronousRequestError {
  1: RequestErrorStruct request_error;
  2: InternalErrorStruct internal_error;
}

struct PushrebaseConflict {
  1: Path left;
  2: Path right;
}

permanent client exception PushrebaseConflictsException {
  @thrift.ExceptionMessage
  1: string reason;
  /// Always non-empty
  2: list<PushrebaseConflict> conflicts;
}

struct HookRejection {
  /// The hook that rejected the output
  1: string hook_name;
  /// The changeset that was reject, in bonsai format.
  2: binary cs_id;
  /// Why the hook rejected the changeset.
  3: HookOutcomeRejected reason;
}

stateful client exception HookRejectionsException {
  @thrift.ExceptionMessage
  1: string reason;
  /// Always non-empty
  2: list<HookRejection> rejections;
}

/// Service Definition

@rust.RequestContext
@thrift.DeprecatedUnvalidatedAnnotations{
  items = {"sr.service_name": "mononoke-scs-server"},
}
service SourceControlService extends fb303_core.BaseService {
  /// Global methods
  /// ==============

  /// Get a list of all repositories.
  list<Repo> list_repos(1: ListReposParams params) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Repository methods
  /// ==================

  /// Get repo info
  RepoInfo repo_info(1: RepoSpecifier repo, 2: RepoInfoParams params) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Resolve a bookmark
  /// The return value may be slightly stale, the served value is only updated
  /// once all the data for new commits is generated and cache warm.
  RepoResolveBookmarkResponse repo_resolve_bookmark(
    1: RepoSpecifier repo,
    2: RepoResolveBookmarkParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Resolve commit by the given prefix
  RepoResolveCommitPrefixResponse repo_resolve_commit_prefix(
    1: RepoSpecifier repo,
    2: RepoResolveCommitPrefixParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Comprehensive information about bookmark (use repo_resolve_bookmark for
  /// simply resolving bookmark value).
  RepoBookmarkInfoResponse repo_bookmark_info(
    1: RepoSpecifier repo,
    2: RepoBookmarkInfoParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// List all bookmarks in the repo.
  RepoListBookmarksResponse repo_list_bookmarks(
    1: RepoSpecifier repo,
    2: RepoListBookmarksParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Generate commit info for all the draft commits
  /// for the given set of heads.and public roots.
  RepoStackInfoResponse repo_stack_info(
    1: RepoSpecifier repo,
    2: RepoStackInfoParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Generate Git bundle for the given stack of commits with the ref BUNDLE_HEAD
  /// pointing to the top of the stack. Store the bundle in everstore and return
  /// the everstore handle associated with it.
  RepoStackGitBundleStoreResponse repo_stack_git_bundle_store(
    1: RepoSpecifier repo,
    2: RepoStackGitBundleStoreParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Repository write methods
  /// ========================

  /// Create a new (draft) commit.
  /// IMPORTANT: This method doesn't push the commit to any bookmark. To get the
  /// commit to mainline you should use this method followed by repo_land_stack.
  RepoCreateCommitResponse repo_create_commit(
    1: RepoSpecifier repo,
    2: RepoCreateCommitParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Create a stack of new commits.  A stack is a linear chain of commits
  /// where each commit is the single immediate child of the previous commit.
  RepoCreateStackResponse repo_create_stack(
    1: RepoSpecifier repo,
    2: RepoCreateStackParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Create a bookmark.
  RepoCreateBookmarkResponse repo_create_bookmark(
    1: RepoSpecifier repo,
    2: RepoCreateBookmarkParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Move a bookmark.
  RepoMoveBookmarkResponse repo_move_bookmark(
    1: RepoSpecifier repo,
    2: RepoMoveBookmarkParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Delete a bookmark.
  RepoDeleteBookmarkResponse repo_delete_bookmark(
    1: RepoSpecifier repo,
    2: RepoDeleteBookmarkParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Land a stack of commits via pushrebase.
  RepoLandStackResponse repo_land_stack(
    1: RepoSpecifier repo,
    2: RepoLandStackParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: PushrebaseConflictsException pushrebase_conflicts,
    4: HookRejectionsException hook_rejections,
    5: OverloadError overload_error,
  );

  /// Derive data for commits in a repo
  RepoPrepareCommitsResponse repo_prepare_commits(
    1: RepoSpecifier repo,
    2: RepoPrepareCommitsParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Upload new file content
  RepoUploadFileContentResponse repo_upload_file_content(
    1: RepoSpecifier repo,
    2: RepoUploadFileContentParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Commit methods
  /// ==============

  /// Diff paths in commit against paths in other commits.
  /// NOTE: Works only on files, doesn't diff directories.
  /// NOTE2: There are limits on how many files you can diff at once:
  ///  * at most COMMIT_DIFF_FILES_PATH_COUNT_LIMIT paths can be requested
  ///  * the total size of diffed files must be less than
  ///    COMMIT_DIFF_FILES_SIZE_LIMIT
  CommitFileDiffsResponse commit_file_diffs(
    1: CommitSpecifier commit,
    2: CommitFileDiffsParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Look-up a commit to see if it exists and find alternative IDs.
  CommitLookupResponse commit_lookup(
    1: CommitSpecifier commit,
    2: CommitLookupParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Look-up a list of commits to see if they exists and find alternative IDs.
  /// Note that this API does not trigger git commit derivation.
  RepoMultipleCommitLookupResponse repo_multiple_commit_lookup(
    1: RepoSpecifier repo,
    2: RepoMultipleCommitLookupParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Look up commit history over Pushrebase mutations. It finishes on commit
  /// version that was originally pushed. Provided commit must be public.
  ///
  /// Currently attempts to traverse over commit sync and pushrebase mappings.
  /// Returns an error if there is ambiguity in any mapping but this is not
  /// expected to ever happen.
  ///
  /// The method may return incorrect results for older commits because we
  /// can't backfill the necessary data. "Incorrect" means it will still be
  /// some version of the provided commit but not its true origin.
  ///
  /// NOTE: returns commit specifiers with bonsai hashes. Use commit_lookup
  /// on specifiers to obtain hashes in needed schemes.
  CommitLookupPushrebaseHistoryResponse commit_lookup_pushrebase_history(
    1: CommitSpecifier commit,
    2: CommitLookupPushrebaseHistoryParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Get commit info.
  CommitInfo commit_info(
    1: CommitSpecifier commit,
    2: CommitInfoParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Get commit generation.
  i64 commit_generation(
    1: CommitSpecifier commit,
    2: CommitGenerationParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Check if this commit is an ancestor of some other commit.
  bool commit_is_ancestor_of(
    1: CommitSpecifier commit,
    2: CommitIsAncestorOfParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Find the lowest common ancestor of two commits.
  ///
  /// In case of ambiguity (can happen with multiple merges of the same
  /// branches) returns the common ancestor with lowest hash out of those with
  /// highest generation number.
  CommitLookupResponse commit_common_base_with(
    1: CommitSpecifier commit,
    2: CommitCommonBaseWithParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Compute differences between two commits.
  /// note: copy/move information included only when comparing with parent
  CommitCompareResponse commit_compare(
    1: CommitSpecifier commit,
    2: CommitCompareParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Find files within the commit that match criteria.
  CommitFindFilesResponse commit_find_files(
    1: CommitSpecifier commit,
    2: CommitFindFilesParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Find files within the commit that match criteria.
  CommitFindFilesStreamResponse, stream<
    CommitFindFilesStreamItem throws (
      1: RequestError request_error,
      2: InternalError internal_error,
      3: OverloadError overload_error,
    )
  > commit_find_files_stream(
    1: CommitSpecifier commit,
    2: CommitFindFilesParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CommitHistoryResponse commit_history(
    1: CommitSpecifier commit,
    2: CommitHistoryParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CommitLinearHistoryResponse commit_linear_history(
    1: CommitSpecifier commit,
    2: CommitLinearHistoryParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CommitListDescendantBookmarksResponse commit_list_descendant_bookmarks(
    1: CommitSpecifier commit,
    2: CommitListDescendantBookmarksParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Run hooks for a commit without landing it. Useful for getting early signal.
  /// It is NOT guaranteed that a push will succeed if all hooks pass,
  /// as things other than hooks can fail - e.g. rebase failures.
  CommitRunHooksResponse commit_run_hooks(
    1: CommitSpecifier commit,
    2: CommitRunHooksParams params,
  ) throws (
    1: RequestError request_error,
    // @lint-ignore SPELL (this was not caught by the previous SPELL linter)
    2: InternalError interal_error,
    3: OverloadError overload_error,
  );

  CommitSubtreeChangesResponse commit_subtree_changes(
    1: CommitSpecifier commit,
    2: CommitSubtreeChangesParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CommitHgMutationHistoryResponse commit_hg_mutation_history(
    1: CommitSpecifier commit,
    2: CommitHgMutationHistoryParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// CommitPath methods
  /// ==============

  /// Determine whether a path exists and what type it is.
  CommitPathExistsResponse commit_path_exists(
    1: CommitPathSpecifier commit_path,
    2: CommitPathExistsParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Get information about a path in a commit.
  CommitPathInfoResponse commit_path_info(
    1: CommitPathSpecifier commit_path,
    2: CommitPathInfoParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Get information about multiple paths in a commit.
  CommitMultiplePathInfoResponse commit_multiple_path_info(
    1: CommitSpecifier commit,
    2: CommitMultiplePathInfoParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CommitPathBlameResponse commit_path_blame(
    1: CommitPathSpecifier commit_path,
    2: CommitPathBlameParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CommitPathHistoryResponse commit_path_history(
    1: CommitPathSpecifier commit_path,
    2: CommitPathHistoryParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CommitPathLastChangedResponse commit_path_last_changed(
    1: CommitPathSpecifier commit_path,
    2: CommitPathLastChangedParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CommitMultiplePathLastChangedResponse commit_multiple_path_last_changed(
    1: CommitSpecifier commit,
    2: CommitMultiplePathLastChangedParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Calculate the size change for each sparse profile for a given commit
  CommitSparseProfileDeltaToken commit_sparse_profile_delta_async(
    1: CommitSparseProfileDeltaParamsV2 params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Poll for completion of a commit_sparse_profile_delta_async call.
  CommitSparseProfileDeltaPollResponse commit_sparse_profile_delta_poll(
    1: CommitSparseProfileDeltaToken token,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
    4: PollError poll_error,
  );

  /// Calculate the total size of each sparse profiles
  CommitSparseProfileSizeToken commit_sparse_profile_size_async(
    1: CommitSparseProfileSizeParamsV2 params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Poll for completion of a commit_sparse_profile_size_async call.
  CommitSparseProfileSizePollResponse commit_sparse_profile_size_poll(
    1: CommitSparseProfileSizeToken token,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
    4: PollError poll_error,
  );

  /// Tree Methods
  /// ============

  /// Determine whether a tree exists.
  bool tree_exists(1: TreeSpecifier file, 2: TreeExistsParams params) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// List the contents of a directory.
  TreeListResponse tree_list(
    1: TreeSpecifier tree,
    2: TreeListParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// File Methods
  /// ============

  /// Determine whether a file exists.
  bool file_exists(1: FileSpecifier file, 2: FileExistsParams params) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Get information about a file.
  FileInfo file_info(1: FileSpecifier file, 2: FileInfoParams params) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Get a chunk of a file's content.
  FileChunk file_content_chunk(
    1: FileSpecifier file,
    2: FileContentChunkParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Compare a file with another file.
  FileDiffResponse file_diff(
    1: FileSpecifier file,
    2: FileDiffParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Cross-Repo Methods
  /// ============================

  /// Look-up a commit to find its identity (if any) in another repo
  CommitLookupResponse commit_lookup_xrepo(
    1: CommitSpecifier commit,
    2: CommitLookupXRepoParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Repository management methods
  /// ==============================

  CreateReposToken create_repos(1: CreateReposParams params) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CreateReposPollResponse create_repos_poll(1: CreateReposToken token) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Megarepo Service Methods
  /// ========================

  /// Add a new unused config version to the library of versions
  MegarepoAddConfigResponse megarepo_add_sync_target_config(
    1: MegarepoAddConfigParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Read the target config for a particular commit
  MegarepoReadConfigResponse megarepo_read_target_config(
    1: MegarepoReadConfigParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Add a new target to the list of known targets and set its
  /// initial SyncTargetConfig value
  MegarepoAddTargetToken megarepo_add_sync_target(
    1: MegarepoAddTargetParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  //// Poll the execution of megarepo_add_sync_target request
  MegarepoAddTargetPollResponse megarepo_add_sync_target_poll(
    1: MegarepoAddTargetToken token,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Add a new target that branches off existing target.
  MegarepoAddBranchingTargetToken megarepo_add_branching_sync_target(
    1: MegarepoAddBranchingTargetParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  //// Poll the execution of megarepo_add_sync_target request
  MegarepoAddBranchingTargetPollResponse megarepo_add_branching_sync_target_poll(
    1: MegarepoAddBranchingTargetToken token,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Set target's version to a new value while applying necessary transformations
  /// These transformations may include:
  /// - deletions of old mappings of sources that need to be re-merged
  /// - transformation-applying changesets on sources
  /// - re-merges of sources
  /// Returns a new position of the target bookmark
  /// Note: may advance the bookmark by >1 commit
  MegarepoChangeConfigToken megarepo_change_target_config(
    1: MegarepoChangeTargetConfigParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Poll the execution of change_target_config request
  MegarepoChangeTargetConfigPollResponse megarepo_change_target_config_poll(
    1: MegarepoChangeConfigToken token,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Sync commits up until cs_id source -> target
  MegarepoSyncChangesetToken megarepo_sync_changeset(
    1: MegarepoSyncChangesetParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Poll the execution of sync_changeset request
  MegarepoSyncChangesetPollResponse megarepo_sync_changeset_poll(
    1: MegarepoSyncChangesetToken token,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Merge source into target from the `cs_id`,
  /// removing previous remapping of the source
  /// Returns a new position of the target bookmark
  /// This is how we handle non-forward bookmark moves
  /// in small repos
  /// Note: source may have moved from cs_id since
  ///       this function was called, this function
  ///       will merge `cs_id` into target
  /// Note: this fn will only succeed if target points
  ///       to `target_location` by the time the fn
  ///       advances the target
  MegarepoRemergeSourceToken megarepo_remerge_source(
    1: MegarepoRemergeSourceParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Poll the execution of megarepo_re_merge_source request
  MegarepoRemergeSourcePollResponse megarepo_remerge_source_poll(
    1: MegarepoRemergeSourceToken token,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  RepoUpdateSubmoduleExpansionResponse repo_update_submodule_expansion(
    1: RepoUpdateSubmoduleExpansionParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Git Import Methods
  /// ==================

  /// Upload raw git object to Mononoke data store for back-and-forth translation.
  /// Not to be used for uploading raw file content blobs.
  RepoUploadNonBlobGitObjectResponse repo_upload_non_blob_git_object(
    1: RepoSpecifier repo,
    2: RepoUploadNonBlobGitObjectParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Upload packfile base item corresponding to git object to Mononoke data store
  RepoUploadPackfileBaseItemResponse repo_upload_packfile_base_item(
    1: RepoSpecifier repo,
    2: RepoUploadPackfileBaseItemParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Create Mononoke counterpart of git tree object in the form of a bonsai changeset.
  /// The raw git tree object must already be stored in Mononoke stores before invoking
  /// this endpoint.
  CreateGitTreeResponse create_git_tree(
    1: RepoSpecifier repo,
    2: CreateGitTreeParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Create Mononoke counterpart of git tag object in the form of a bonsai changeset.
  /// The raw git tag object must already be stored in Mononoke stores before invoking
  /// this endpoint.
  CreateGitTagResponse create_git_tag(
    1: RepoSpecifier repo,
    2: CreateGitTagParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Commit Cloud Methods
  /// ==================

  /// Get general info of a commit cloud workspace
  CloudWorkspaceInfoResponse cloud_workspace_info(
    1: CloudWorkspaceInfoParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Get general info about all the workspaces associated with a user
  CloudUserWorkspacesResponse cloud_user_workspaces(
    1: CloudUserWorkspacesParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  CloudWorkspaceSmartlogResponse cloud_workspace_smartlog(
    1: CloudWorkspaceSmartlogParams params,
  ) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  /// Test support methods
  /// ==================

  AsyncPingToken async_ping(1: AsyncPingParams params) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
  );

  //// Poll the execution of async_ping request
  AsyncPingPollResponse async_ping_poll(1: AsyncPingToken token) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
    3: OverloadError overload_error,
    4: PollError poll_error,
  );
}
