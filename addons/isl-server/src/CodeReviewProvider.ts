/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TypeaheadKind, TypeaheadResult} from 'isl/src/CommitInfoView/types';
import type {
  DiffId,
  DiffSummary,
  Disposable,
  Result,
  OperationCommandProgressReporter,
  LandInfo,
  LandConfirmationInfo,
  CodeReviewProviderSpecificClientToServerMessages,
  ClientToServerMessage,
} from 'isl/src/types';

type DiffSummaries = Map<DiffId, DiffSummary>;
/**
 * API to fetch data from Remote Code Review system, like GitHub and Phabricator.
 */
export interface CodeReviewProvider {
  triggerDiffSummariesFetch(diffs: Array<DiffId>): unknown;

  onChangeDiffSummaries(callback: (result: Result<DiffSummaries>) => unknown): Disposable;

  /** Run a command not handled within sapling, such as a separate submit handler */
  runExternalCommand?(
    cwd: string,
    args: Array<string>,
    onProgress: OperationCommandProgressReporter,
    signal: AbortSignal,
  ): Promise<void>;

  dispose: () => void;

  /** Convert Code Review Provider info into a short summary string, usable in analytics */
  getSummaryName(): string;

  typeahead?(kind: TypeaheadKind, query: string): Promise<Array<TypeaheadResult>>;

  getDiffUrlMarkdown(diffId: DiffId): string;
  getCommitHashUrlMarkdown(hash: string): string;

  updateDiffMessage?(diffId: DiffId, newTitle: string, newDescription: string): Promise<void>;

  getSuggestedReviewers?(context: {paths: Array<string>}): Promise<Array<string>>;

  /** Convert usernames/emails to avatar URIs */
  fetchAvatars?(authors: Array<string>): Promise<Map<string, string>>;

  renderMarkup?: (markup: string) => Promise<string>;

  fetchLandInfo?(topOfStack: DiffId): Promise<LandInfo>;
  confirmLand?(landConfirmationInfo: NonNullable<LandConfirmationInfo>): Promise<Result<undefined>>;

  handleClientToServerMessage?(
    message: ClientToServerMessage,
  ): message is CodeReviewProviderSpecificClientToServerMessages;
}
