/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TypeaheadResult} from 'isl-components/Types';
import type {TypeaheadKind} from 'isl/src/CommitInfoView/types';
import type {
  ClientToServerMessage,
  CodeReviewProviderSpecificClientToServerMessages,
  CommandArg,
  DiffComment,
  DiffId,
  DiffSummary,
  Disposable,
  LandConfirmationInfo,
  LandInfo,
  OperationCommandProgressReporter,
  Result,
  ServerToClientMessage,
} from 'isl/src/types';

export type DiffSummaries = Map<DiffId, DiffSummary>;
/**
 * API to fetch data from Remote Code Review system, like GitHub and Phabricator.
 */
export interface CodeReviewProvider {
  /**
   * For when something moved that the inputs don't show. Where it is honored, `force` skips
   * whatever the provider would otherwise serve from its own state, such as debouncing.
   *
   * `partial` says `diffs` names particular diffs of interest rather than every diff on screen, so
   * a provider that remembers what to refetch later does not mistake it for the whole set.
   *
   * The two together mean "this diff moved", which is not enough to discard state held about the
   * others: the Phabricator provider skips the cache-wide invalidation it does for `force` alone,
   * and since that cache has no per-diff eviction, the named diffs' cached counts survive too.
   *
   * Both are requests rather than guarantees: the GitHub provider takes no arguments at all and
   * stays on its own debounce.
   */
  triggerDiffSummariesFetch(diffs: Array<DiffId>, force?: boolean, partial?: boolean): unknown;

  onChangeDiffSummaries(callback: (result: Result<DiffSummaries>) => unknown): Disposable;

  /** Run a command not handled within sapling, such as a separate submit handler */
  runExternalCommand?(
    cwd: string,
    args: CommandArg[], // Providers may need specific normalization for args
    onProgress: OperationCommandProgressReporter,
    signal: AbortSignal,
  ): Promise<void>;

  /** Run a conf command for configerator operations */
  runConfCommand?(
    cwd: string,
    args: Array<string>,
    onProgress: OperationCommandProgressReporter,
    signal: AbortSignal,
  ): Promise<void>;

  dispose: () => void;

  /** Convert Code Review Provider info into a short summary string, usable in analytics */
  getSummaryName(): string;

  typeahead?(kind: TypeaheadKind, query: string, cwd: string): Promise<Array<TypeaheadResult>>;

  getDiffUrlMarkdown(diffId: DiffId): string;
  getCommitHashUrlMarkdown(hash: string): string;

  getRemoteFileURL?(
    path: string,
    publicCommitHash: string | null,
    selectionStart?: {line: number; char: number},
    selectionEnd?: {line: number; char: number},
  ): string;

  updateDiffMessage?(diffId: DiffId, newTitle: string, newDescription: string): Promise<void>;

  getSuggestedReviewers?(context: {paths: Array<string>}): Promise<Array<string>>;

  /** Convert usernames/emails to avatar URIs */
  fetchAvatars?(authors: Array<string>): Promise<Map<string, string>>;

  /** Convert usernames/emails to avatar URIs */
  fetchComments?(diffId: DiffId): Promise<Array<DiffComment>>;

  renderMarkup?: (markup: string) => Promise<string>;

  /** Fetch commit hashes of the user's authored open diffs */
  fetchAuthoredDiffs?(): Promise<Array<string>>;

  fetchLandInfo?(topOfStack: DiffId): Promise<LandInfo>;
  confirmLand?(landConfirmationInfo: NonNullable<LandConfirmationInfo>): Promise<Result<undefined>>;

  handleClientToServerMessage?(
    message: ClientToServerMessage,
    postMessage: (message: ServerToClientMessage) => void,
  ): message is CodeReviewProviderSpecificClientToServerMessages;
}
