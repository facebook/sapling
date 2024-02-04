/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitPreview, WithPreviewType} from '../previews';
import type {
  ChangedFile,
  CommitInfo,
  CommitPhaseType,
  Hash,
  StableCommitMetadata,
  SuccessorInfo,
} from '../types';
import type {RecordOf, List} from 'immutable';

import {Record} from 'immutable';
import {SelfUpdate} from 'shared/immutableExt';

type DagExt = {
  /** Distance ancestors that are treated as direct parents. */
  ancestors?: List<Hash>;

  /**
   * Insertion batch. Larger: later inserted.
   * All 'sl log' commits share a same initial number.
   * Later previews might have larger numbers.
   * Used for sorting.
   */
  seqNumber?: number;

  /** If true, this is a virtual "You are here" commit. */
  isYouAreHere?: boolean;
};

// Note: There are some non-immutable containers (Array) in `CommitInfo`
// such as bookmarks. Since the "commitInfos" are "normalized" by
// `reuseEqualObjects`. Those non-immutable properties should still
// compare fine.
type CommitInfoExtProps = CommitInfo & WithPreviewType & DagExt;

const CommitInfoExtRecord = Record<CommitInfoExtProps>({
  title: '',
  hash: '',
  parents: [],
  phase: 'draft',
  isHead: false,
  author: '',
  date: new Date(0),
  description: '',
  bookmarks: [],
  remoteBookmarks: [],
  successorInfo: undefined,
  closestPredecessors: undefined,
  filesSample: [],
  totalFileCount: 0,
  diffId: undefined,
  stableCommitMetadata: undefined,

  // WithPreviewType
  previewType: undefined,

  // DagExt
  ancestors: undefined,
  seqNumber: undefined,
  isYouAreHere: undefined,
});
type CommitInfoExtRecord = RecordOf<CommitInfoExtProps>;

/** Immutable, extended `CommitInfo` */
export class DagCommitInfo extends SelfUpdate<CommitInfoExtRecord> {
  constructor(record: CommitInfoExtRecord) {
    super(record);
  }

  static fromCommitInfo(info: Partial<CommitInfoExtProps>): DagCommitInfo {
    const record = CommitInfoExtRecord(info);
    return new DagCommitInfo(record);
  }

  // Immutable.js APIs

  set<K extends keyof CommitInfoExtProps>(key: K, value: CommitInfoExtProps[K]): DagCommitInfo {
    return new DagCommitInfo(this.inner.set(key, value));
  }

  withMutations(mutator: (mutable: CommitInfoExtRecord) => CommitInfoExtRecord) {
    const record = this.inner.withMutations(mutator);
    return new DagCommitInfo(record);
  }

  merge(
    ...collections: Array<Partial<CommitInfoExtProps> | Iterable<[string, unknown]>>
  ): DagCommitInfo {
    return this.withMutations(m => m.merge(...collections));
  }

  // Getters

  public get title(): string {
    return this.inner.title;
  }

  public get hash(): Hash {
    return this.inner.hash;
  }

  public get parents(): ReadonlyArray<Hash> {
    return this.inner.parents;
  }

  get phase(): CommitPhaseType {
    return this.inner.phase;
  }

  get isHead(): boolean {
    return this.inner.isHead;
  }

  get author(): string {
    return this.inner.author;
  }

  get date(): Date {
    return this.inner.date;
  }

  get description(): string {
    return this.inner.description;
  }

  get bookmarks(): ReadonlyArray<string> {
    return this.inner.bookmarks;
  }

  get remoteBookmarks(): ReadonlyArray<string> {
    return this.inner.remoteBookmarks;
  }

  get successorInfo(): Readonly<SuccessorInfo> | undefined {
    return this.inner.successorInfo;
  }

  get closestPredecessors(): ReadonlyArray<Hash> | undefined {
    return this.inner.closestPredecessors;
  }

  get filesSample(): ReadonlyArray<ChangedFile> {
    return this.inner.filesSample;
  }

  get totalFileCount(): number {
    return this.inner.totalFileCount;
  }

  get diffId(): string | undefined {
    return this.inner.diffId;
  }

  get stableCommitMetadata(): ReadonlyArray<StableCommitMetadata> | undefined {
    return this.inner.stableCommitMetadata;
  }

  get previewType(): CommitPreview | undefined {
    return this.inner.previewType;
  }

  get ancestors(): List<Hash> | undefined {
    return this.inner.ancestors;
  }

  get seqNumber(): number | undefined {
    return this.inner.seqNumber;
  }

  get isYouAreHere(): boolean | undefined {
    return this.inner.isYouAreHere;
  }
}
