/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from './logger';
import type {
  ChangedFile,
  CommitInfo,
  CommitPhaseType,
  Hash,
  RepoRelativePath,
  ShelvedChange,
  SmartlogCommits,
  StableCommitFetchConfig,
  StableInfo,
  SuccessorInfo,
} from 'isl/src/types';

import {Internal} from './Internal';
import {MAX_FETCHED_FILES_PER_COMMIT} from './commands';
import {fromEntries} from './utils';
import path from 'path';

export const COMMIT_END_MARK = '<<COMMIT_END_MARK>>';
export const NULL_CHAR = '\0';
export const ESCAPED_NULL_CHAR = '\\0';
export const WDIR_PARENT_MARKER = '@';

///// Main commits fetch /////

export const FIELDS = {
  hash: '{node}',
  title: '{desc|firstline}',
  author: '{author}',
  // We prefer committerdate over authordate as authordate sometimes makes
  // amended or rebased commits look stale
  date: '{committerdate|isodatesec}',
  phase: '{phase}',
  bookmarks: `{bookmarks % '{bookmark}${ESCAPED_NULL_CHAR}'}`,
  remoteBookmarks: `{remotenames % '{remotename}${ESCAPED_NULL_CHAR}'}`,
  parents: `{parents % "{node}${ESCAPED_NULL_CHAR}"}`,
  isDot: `{ifcontains(rev, revset('.'), '${WDIR_PARENT_MARKER}')}`,
  filesAdded: '{file_adds|json}',
  filesModified: '{file_mods|json}',
  filesRemoved: '{file_dels|json}',
  successorInfo: '{mutations % "{operation}:{successors % "{node}"},"}',
  closestPredecessors: '{predecessors % "{node},"}',
  // This would be more elegant as a new built-in template
  diffId: '{if(phabdiff, phabdiff, github_pull_request_number)}',
  isFollower: '{sapling_pr_follower|json}',
  stableCommitMetadata: Internal.stableCommitConfig?.template ?? '',
  // Description must be last
  description: '{desc}',
};

export const FIELD_INDEX = fromEntries(Object.keys(FIELDS).map((key, i) => [key, i])) as {
  [key in Required<keyof typeof FIELDS>]: number;
};
export const FETCH_TEMPLATE = [...Object.values(FIELDS), COMMIT_END_MARK].join('\n');

/**
 * Extract CommitInfos from log calls that use FETCH_TEMPLATE.
 */
export function parseCommitInfoOutput(
  logger: Logger,
  output: string,
  stableCommitConfig = Internal.stableCommitConfig as StableCommitFetchConfig | null,
): SmartlogCommits {
  const revisions = output.split(COMMIT_END_MARK);
  const commitInfos: Array<CommitInfo> = [];
  for (const chunk of revisions) {
    try {
      const lines = chunk.trimStart().split('\n');
      if (lines.length < Object.keys(FIELDS).length) {
        continue;
      }
      const files: Array<ChangedFile> = [
        ...(JSON.parse(lines[FIELD_INDEX.filesModified]) as Array<string>).map(path => ({
          path,
          status: 'M' as const,
        })),
        ...(JSON.parse(lines[FIELD_INDEX.filesAdded]) as Array<string>).map(path => ({
          path,
          status: 'A' as const,
        })),
        ...(JSON.parse(lines[FIELD_INDEX.filesRemoved]) as Array<string>).map(path => ({
          path,
          status: 'R' as const,
        })),
      ];

      // Find if the commit is entirely within the cwd and therefore mroe relevant to the user.
      // Note: this must be done on the server using the full list of files, not just the sample that the client gets.
      // TODO: should we cache this by commit hash to avoid iterating all files on the same commits every time?
      const maxCommonPathPrefix = findMaxCommonPathPrefix(files);

      commitInfos.push({
        hash: lines[FIELD_INDEX.hash],
        title: lines[FIELD_INDEX.title],
        author: lines[FIELD_INDEX.author],
        date: new Date(lines[FIELD_INDEX.date]),
        parents: splitLine(lines[FIELD_INDEX.parents]) as string[],
        phase: lines[FIELD_INDEX.phase] as CommitPhaseType,
        bookmarks: splitLine(lines[FIELD_INDEX.bookmarks]),
        remoteBookmarks: splitLine(lines[FIELD_INDEX.remoteBookmarks]),
        isDot: lines[FIELD_INDEX.isDot] === WDIR_PARENT_MARKER,
        filesSample: files.slice(0, MAX_FETCHED_FILES_PER_COMMIT),
        totalFileCount: files.length,
        successorInfo: parseSuccessorData(lines[FIELD_INDEX.successorInfo]),
        closestPredecessors: splitLine(lines[FIELD_INDEX.closestPredecessors], ','),
        description: lines
          .slice(FIELD_INDEX.description + 1 /* first field of description is title; skip it */)
          .join('\n')
          .trim(),
        diffId: lines[FIELD_INDEX.diffId] != '' ? lines[FIELD_INDEX.diffId] : undefined,
        isFollower: JSON.parse(lines[FIELD_INDEX.isFollower]) as boolean,
        stableCommitMetadata:
          lines[FIELD_INDEX.stableCommitMetadata] != ''
            ? stableCommitConfig?.parse(lines[FIELD_INDEX.stableCommitMetadata])
            : undefined,
        maxCommonPathPrefix,
      });
    } catch (err) {
      logger.error('failed to parse commit');
    }
  }
  return commitInfos;
}

/**
 * Given a set of changed files, find the longest common path prefix.
 * See {@link CommitInfo}.maxCommonPathPrefix
 * TODO: This could be cached by commit hash
 */
export function findMaxCommonPathPrefix(files: Array<ChangedFile>): RepoRelativePath {
  let max: null | Array<string> = null;
  let maxLength = 0;

  // Path module separator should match what `sl` gives us
  const sep = path.sep;

  for (const file of files) {
    if (max == null) {
      max = file.path.split(sep);
      max.pop(); // ignore file part, only care about directory
      maxLength = max.reduce((acc, part) => acc + part.length + 1, 0); // +1 for slash
      continue;
    }
    // small optimization: we only need to look as long as the max so far, max common path will always be shorter
    const parts = file.path.slice(0, maxLength).split(sep);
    for (const [i, part] of parts.entries()) {
      if (part !== max[i]) {
        max = max.slice(0, i);
        maxLength = max.reduce((acc, part) => acc + part.length + 1, 0); // +1 for slash
        break;
      }
    }
    if (max.length === 0) {
      return ''; // we'll never get *more* specific, early exit
    }
  }

  const result = (max ?? []).join(sep);
  if (result == '') {
    return result;
  }
  return result + sep;
}

/**
 * Additional stable locations in the commit fetch will not automatically
 * include "stableCommitMetadata". Insert this data onto the commits.
 */
export function attachStableLocations(commits: Array<CommitInfo>, locations: Array<StableInfo>) {
  const map: Record<Hash, Array<StableInfo>> = {};
  for (const location of locations) {
    const existing = map[location.hash] ?? [];
    map[location.hash] = [...existing, location];
  }

  for (const commit of commits) {
    if (commit.hash in map) {
      commit.stableCommitMetadata = [
        ...(commit.stableCommitMetadata ?? []),
        ...map[commit.hash].map(location => ({
          value: location.name,
          description: location.info ?? '',
        })),
      ];
    }
  }
}

///// Shelve /////

export const SHELVE_FIELDS = {
  hash: '{node}',
  name: '{shelvename}',
  author: '{author}',
  date: '{date|isodatesec}',
  filesAdded: '{file_adds|json}',
  filesModified: '{file_mods|json}',
  filesRemoved: '{file_dels|json}',
  description: '{desc}',
};
export const SHELVE_FIELD_INDEX = fromEntries(
  Object.keys(SHELVE_FIELDS).map((key, i) => [key, i]),
) as {
  [key in Required<keyof typeof SHELVE_FIELDS>]: number;
};
export const SHELVE_FETCH_TEMPLATE = [...Object.values(SHELVE_FIELDS), COMMIT_END_MARK].join('\n');

export function parseShelvedCommitsOutput(logger: Logger, output: string): Array<ShelvedChange> {
  const shelves = output.split(COMMIT_END_MARK);
  const commitInfos: Array<ShelvedChange> = [];
  for (const chunk of shelves) {
    try {
      const lines = chunk.trim().split('\n');
      if (lines.length < Object.keys(SHELVE_FIELDS).length) {
        continue;
      }
      const files: Array<ChangedFile> = [
        ...(JSON.parse(lines[SHELVE_FIELD_INDEX.filesModified]) as Array<string>).map(path => ({
          path,
          status: 'M' as const,
        })),
        ...(JSON.parse(lines[SHELVE_FIELD_INDEX.filesAdded]) as Array<string>).map(path => ({
          path,
          status: 'A' as const,
        })),
        ...(JSON.parse(lines[SHELVE_FIELD_INDEX.filesRemoved]) as Array<string>).map(path => ({
          path,
          status: 'R' as const,
        })),
      ];
      commitInfos.push({
        hash: lines[SHELVE_FIELD_INDEX.hash],
        name: lines[SHELVE_FIELD_INDEX.name],
        date: new Date(lines[SHELVE_FIELD_INDEX.date]),
        filesSample: files.slice(0, MAX_FETCHED_FILES_PER_COMMIT),
        totalFileCount: files.length,
        description: lines.slice(SHELVE_FIELD_INDEX.description).join('\n'),
      });
    } catch (err) {
      logger.error('failed to parse shelved change');
    }
  }
  return commitInfos;
}

///// Changed Files /////

export const CHANGED_FILES_FIELDS = {
  hash: '{node}',
  filesAdded: '{file_adds|json}',
  filesModified: '{file_mods|json}',
  filesRemoved: '{file_dels|json}',
};
export const CHANGED_FILES_INDEX = fromEntries(
  Object.keys(CHANGED_FILES_FIELDS).map((key, i) => [key, i]),
) as {
  [key in Required<keyof typeof CHANGED_FILES_FIELDS>]: number;
};
export const CHANGED_FILES_TEMPLATE = [
  ...Object.values(CHANGED_FILES_FIELDS),
  COMMIT_END_MARK,
].join('\n');

///// Helpers /////

function parseSuccessorData(successorData: string): SuccessorInfo | undefined {
  const [successorString] = successorData.split(',', 1); // we're only interested in the first available mutation
  if (!successorString) {
    return undefined;
  }
  const successor = successorString.split(':');
  return {
    hash: successor[1],
    type: successor[0],
  };
}
function splitLine(line: string, separator = NULL_CHAR): Array<string> {
  return line.split(separator).filter(e => e.length > 0);
}
