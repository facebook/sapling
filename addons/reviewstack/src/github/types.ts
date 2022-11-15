/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PageInfo, Scalars} from '../generated/graphql';

/**
 * Types for GitHubClient API. These roughly map to the types in the
 * GitHub GraphQL API, though there are exceptions. For example, in the GraphQL
 * API, `Commit` has a method named `parents` whereas in our internal API,
 * `parents` is a field.
 */

/** See https://docs.github.com/en/graphql/reference/scalars#id */
export type ID = Scalars['ID'];

/**
 * This is the hex version of the id, not the binary version.
 * See https://docs.github.com/en/graphql/reference/scalars#gitobjectid
 */
export type GitObjectID = Scalars['GitObjectID'];

/**
 * An ISO-8601 encoded UTC date string.
 * See https://docs.github.com/en/graphql/reference/scalars#datetime
 */
export type DateTime = Scalars['DateTime'];

/** See https://docs.github.com/en/graphql/reference/interfaces#gitobject */
export interface GitObject {
  oid: GitObjectID;
}

/** See https://docs.github.com/en/graphql/reference/interfaces#node */
export interface Node {
  id: ID;
}

/** See https://docs.github.com/en/graphql/reference/objects#commit */
export interface Commit extends Node, GitObject {
  /** The datetime when this commit was committed. */
  committedDate: DateTime;

  /** The HTTP URL for this commit. */
  url: string;

  /** The Git commit message. */
  message: string;

  /** The Git commit message headline. */
  messageHeadline: string;

  /** The commit message headline rendered to HTML. */
  messageHeadlineHTML: string;

  /** The Git commit message body. */
  messageBody: string;

  /** The commit message body rendered to HTML. */
  messageBodyHTML: string;

  /** Parents of this commit. */
  parents: GitObjectID[];

  /** This commit's root Tree. */
  tree: Tree;
}

/** See https://docs.github.com/en/graphql/reference/objects#tree */
export interface Tree extends Node, GitObject {
  entries: TreeEntry[];
}

/** See https://docs.github.com/en/graphql/reference/objects#treeentry */
export interface TreeEntry {
  oid: GitObjectID;

  /** Entry file object. */
  object: GitObject | null;

  /** Entry file name. */
  name: string;

  /** The full path of the file. */
  path: string | null;

  /** Entry file mode. */
  mode: number;

  /** Entry file type. */
  type: 'tree' | 'blob';
}

/** See https://docs.github.com/en/graphql/reference/objects#blob */
export interface Blob extends Node, GitObject {
  /** Byte size of Blob object. */
  byteSize: number;

  /**
   * Indicates whether the Blob is binary or text. null if unable to determine
   * the encoding.
   */
  isBinary: boolean | null;

  /** Whether the contents is truncated. */
  isTruncated: boolean;

  /**
   * According to GitHub's GraphQL API, the policy for setting this field is:
   * "UTF8 text data or null if the Blob is binary." For now, we do something
   * slightly different:
   * - !isBinary => UTF8 text data
   * - isBinary && text != null => text is base64-encoded data
   * - isBinary && text == null => we were unable to get the contents
   */
  text: string | null;
}

export interface ForcePushEvent {
  createdAt: DateTime;
  beforeCommit: GitObjectID;
  beforeCommittedDate: DateTime;
  beforeTree: GitObjectID;
  beforeParents: GitObjectID[];
  afterCommit: GitObjectID;
  afterCommittedDate: DateTime;
  afterTree: GitObjectID;
  afterParents: GitObjectID[];
}

/** Commit that belongs to a "version" of a pull request. */
export interface VersionCommit {
  /**
   * Display name for author, if available. Currently, there are no guarantees
   * on the format of this string, as it could be a GitHub username, a person's
   * name, an email address, etc.
   *
   * In practice, all commits that belong to a pull request are authored by the
   * same person, so it is not "mission-critical" to display this information.
   */
  author: string | null;
  commit: GitObjectID;
  committedDate: DateTime;

  /**
   * Effectively the "first line" of the commit message, as this is intended
   * to be displayed in a version selector dropdown, which has minimal screen
   * real estate.
   */
  title: string;
  parents: GitObjectID[];
}

export interface Version {
  headCommit: GitObjectID;
  headCommittedDate: DateTime;
  baseParent: GitObjectID | null;
  baseParentCommittedDate: DateTime | null;
  commits: VersionCommit[];
}

export type PaginationParams =
  | {
      first: number;
      after?: PageInfo['endCursor'];
    }
  | {
      last: number;
      before?: PageInfo['startCursor'];
    };
