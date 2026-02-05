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
  Notification,
  OperationCommandProgressReporter,
  Result,
  ServerToClientMessage,
} from 'isl/src/types';

export type DiffSummaries = Map<DiffId, DiffSummary>;
/**
 * API to fetch data from Remote Code Review system, like GitHub and Phabricator.
 */
export interface CodeReviewProvider {
  triggerDiffSummariesFetch(diffs: Array<DiffId>): unknown;

  /** Set the time range for filtering PRs/diffs. undefined means "all time". */
  setTimeRange?(days: number | undefined): void;

  onChangeDiffSummaries(callback: (result: Result<DiffSummaries>, currentUser?: string) => unknown): Disposable;

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

  /** Reply to an existing comment thread (immediate, not batched) */
  replyToThread?(threadId: string, body: string): Promise<void>;

  /** Resolve a comment thread */
  resolveThread?(threadId: string): Promise<void>;

  /** Unresolve a previously resolved comment thread */
  unresolveThread?(threadId: string): Promise<void>;

  renderMarkup?: (markup: string) => Promise<string>;

  fetchLandInfo?(topOfStack: DiffId): Promise<LandInfo>;
  confirmLand?(landConfirmationInfo: NonNullable<LandConfirmationInfo>): Promise<Result<undefined>>;

  /** Fetch notifications for review requests, mentions, and reviews */
  fetchNotifications?(): Promise<Array<Notification>>;

  handleClientToServerMessage?(
    message: ClientToServerMessage,
    postMessage: (message: ServerToClientMessage) => void,
  ): message is CodeReviewProviderSpecificClientToServerMessages;
}
