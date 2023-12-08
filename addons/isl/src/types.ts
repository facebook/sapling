/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TypeaheadKind, TypeaheadResult} from './CommitInfoView/types';
import type {InternalTypes} from './InternalTypes';
import type {Serializable} from './serialize';
import type {TrackEventName} from 'isl-server/src/analytics/eventNames';
import type {TrackDataWithEventName} from 'isl-server/src/analytics/types';
import type {GitHubDiffSummary} from 'isl-server/src/github/githubCodeReviewProvider';
import type {Comparison} from 'shared/Comparison';
import type {AllUndefined, Json} from 'shared/typeUtils';
import type {Hash} from 'shared/types/common';
import type {ExportStack, ImportedStack, ImportStack} from 'shared/types/stack';

export type Result<T> = {value: T; error?: undefined} | {value?: undefined; error: Error};

/** known supported "Platforms" in which ISL may be embedded */
export type PlatformName =
  | 'browser'
  | 'androidStudio'
  | 'androidStudioRemote'
  | 'vscode'
  | 'standalone'
  | 'webview'
  | 'chromelike_app';

export type AbsolutePath = string;
/**
 * Path relative to repository root dir. Generally, most paths should be RepoRelativePaths,
 * and only convert to CwdRelativePath or basenames or AbsolutePath when needed.
 */
export type RepoRelativePath = string;

/**
 * cwd may be a subdirectory of the repository root.
 * Some commands expect cwd-relative paths,
 * but we generally prefer {@link RepoRelativePaths} when possible. */
export type CwdRelativePath = string;

export type {Hash};

/** Revsets are an eden concept that let you specify commits.
 * This could be a Hash, '.' for HEAD, .^ for parent of head, etc. See `eden help revset` */
export type Revset = string;

/**
 * Diff identifier according to the current repo's remote repository provider (e.g. GitHub)
 * For Github, this is a PR number, like "7" (for PR #7)
 * For Phabricator, this is a Diff number, like "D12345"
 */
export type DiffId = string;
/**
 * "Diff" means a unit of Code Review according to your remote repo provider
 * For GitHub, this is a "Pull Request"
 * For Phabricator, this is a "Diff"
 */

/**
 * Short info about a Diff fetched in bulk for all diffs to render an overview
 */
export type DiffSummary = GitHubDiffSummary | InternalTypes['PhabricatorDiffSummary'];

/**
 * Summary of CI test results for a Diff.
 * 'pass' if ALL signals succeed and not still running.
 * 'failed' if ANY signal doesn't suceed, even if some are still running.
 */
export type DiffSignalSummary = 'running' | 'pass' | 'failed' | 'warning' | 'no-signal';

/**
 * Information about a land request, specific to each Code Review Provider.
 */
export type LandInfo = undefined | InternalTypes['PhabricatorLandInfo'];

/**
 * Information used to confirm a land from a given initiation LandInfo.
 */
export type LandConfirmationInfo = undefined | InternalTypes['PhabricatorLandConfirmationInfo'];

/** An error causing the entire Repository to not be accessible */
export type RepositoryError =
  | {
      type: 'invalidCommand';
      command: string;
    }
  | {type: 'cwdNotARepository'; cwd: string}
  | {
      type: 'unknownError';
      error: Error;
    };

export type RepoInfo = RepositoryError | ValidatedRepoInfo;

/** Proven valid repositories with valid repoRoot / dotdir */
export type ValidatedRepoInfo = {
  type: 'success';
  /** Which cli command name this repository should use for sapling, e.g. `sl`  */
  command: string;
  /**
   * Repo root, which cwd may be a subset of. `undefined` if the cwd is not a valid repository.
   */
  repoRoot: AbsolutePath;
  /**
   * Directory containing sl internal information for this repo, usually `${repoRoot}/.sl`.
   */
  dotdir: AbsolutePath;
  codeReviewSystem: CodeReviewSystem;
  pullRequestDomain: string | undefined;
  preferredSubmitCommand?: PreferredSubmitCommand;
};

export type ApplicationInfo = {
  platformName: string;
  version: string;
  logFilePath: string;
};

export type CodeReviewSystem =
  | {
      type: 'github';
      owner: string;
      repo: string;
      /** github enterprise may use a different hostname than 'github.com' */
      hostname: string;
    }
  | {
      type: 'phabricator';
      repo: string;
    }
  | {
      type: 'none';
    }
  | {
      type: 'unknown';
      path?: string;
    };

export type PreferredSubmitCommand = 'pr' | 'ghstack';

export type StableCommitMetadata = {
  value: string;
  description: string;
};

export type CommitInfo = {
  title: string;
  hash: Hash;
  /**
   * This matches the "parents" information from source control without the
   * "null" hash. Most of the time a commit has 1 parent. For merges there
   * could be 2 or more parents. The initial commit (and initial commits of
   * other merged-in repos) have no parents.
   */
  parents: Hash[];
  phase: CommitPhaseType;
  isHead: boolean;
  author: string;
  date: Date;
  description: string;
  bookmarks: Array<string>;
  remoteBookmarks: Array<string>;
  /** if this commit is obsolete, it is succeeded by another commit */
  successorInfo?: SuccessorInfo;
  /**
   * Closest predecessors (not all recursive predecessors, which can be a long
   * chain and hurt performance). Useful to deal with optimistic states where
   * we know the hashes of predecessors (commits being rewritten) but not their
   * successors (rewritten result).
   *
   * Most of the time a commit only has one predecessor. In case of a fold
   * there are multiple predecessors.
   */
  closestPredecessors?: Hash[];
  /** only a subset of the total files for this commit */
  filesSample: Array<ChangedFile>;
  totalFileCount: number;
  /** @see {@link DiffId} */
  diffId?: DiffId;
  stableCommitMetadata?: Array<StableCommitMetadata>;
};
export type SuccessorInfo = {
  hash: string;
  type: string;
};
export type CommitPhaseType = 'public' | 'draft';
export type ChangedFileType = 'A' | 'M' | 'R' | '?' | '!' | 'U' | 'Resolved';
export type ChangedFile = {
  path: RepoRelativePath;
  status: ChangedFileType;
  /**
   * If this file is copied from another, this is the path of the original file
   * If this file is renamed from another, this is the path of the original file, and another change of type 'R' will exist.
   * */
  copy?: RepoRelativePath;
};

export type SucceedableRevset = {type: 'succeedable-revset'; revset: Revset};
export type ExactRevset = {type: 'exact-revset'; revset: Revset};

/**
 * Most arguments to eden commands are literal `string`s, except:
 * - When specifying file paths, the server needs to know which args are files to convert them to be cwd-relative.
 * - When specifying commit hashes, you may be acting on optimistic version of those hashes.
 *   The server can re-write hashes using a revset that transforms into the latest successor instead.
 *   This allows you to act on the optimistic versions of commits in queued commands,
 *   without a race with the server telling you new versions of those hashes.
 * - If you want an exact commit that's already obsolete or should never be replaced with a succeeded version,
 *   you can use an exact revset.
 * - Specifying config values to override for just this command, so they can be processed separately.
 */
export type CommandArg =
  | string
  | {type: 'repo-relative-file'; path: RepoRelativePath}
  | {type: 'config'; key: string; value: string}
  | ExactRevset
  | SucceedableRevset;

/**
 * What process to execute a given operation in, such as `sl`
 */
export enum CommandRunner {
  Sapling = 'sl',
  /**
   * Use the configured Code Review provider to run this command,
   * such as a non-sapling external submit command
   */
  CodeReviewProvider = 'codeReviewProvider',
  /** Internal arcanist commands */
  InternalArcanist = 'arc',
}

/**
 * {@link CommandArg} representing a hash or revset which should be re-written
 * to the latest successor of that revset when being run.
 * This enables queued commands to act on optimistic state without knowing
 * the optimistic commit's hashes directly.
 */
export function succeedableRevset(revset: Revset): SucceedableRevset {
  return {type: 'succeedable-revset', revset};
}

/**
 * {@link CommandArg} representing a hash or revset which should *not* be re-written
 * to the latest successor of that revset when being run.
 * This uses the revset directly in the command run. Useful if you want to specifically
 * use an obsolete commit in an operation.
 */
export function exactRevset(revset: Revset): ExactRevset {
  return {type: 'exact-revset', revset};
}

/* Subscriptions */

/**
 * A subscription allows the client to ask for a stream of events from the server.
 * The client may send subscribe and corresponding unsubscribe messages.
 * Subscriptions are indexed by a subscriptionId field.
 * Responses to subscriptions are of type Fetched<T>
 */
export type Subscribe<K extends string> =
  | {type: `subscribe${K}`; subscriptionID: string}
  | {type: `unsubscribe${K}`; subscriptionID: string};

/** Reponses to subscriptions, including data and the time duration the fetch lasted */
export type Fetched<K extends string, V> = {
  type: `fetched${K}`;
  subscriptionID: string;
} & V;

export type UncommittedChanges = Array<ChangedFile>;
export type FetchedUncommittedChanges = {
  files: Result<UncommittedChanges>;
  fetchStartTimestamp: number;
  fetchCompletedTimestamp: number;
};

export type BeganFetchingUncommittedChangesEvent = {
  type: 'beganFetchingUncommittedChangesEvent';
};

export type SmartlogCommits = Array<CommitInfo>;
export type FetchedCommits = {
  commits: Result<SmartlogCommits>;
  fetchStartTimestamp: number;
  fetchCompletedTimestamp: number;
};

export type BeganFetchingSmartlogCommitsEvent = {
  type: 'beganFetchingSmartlogCommitsEvent';
};

export type ShelvedChange = {
  hash: Hash;
  name: string;
  date: Date;
  filesSample: Array<ChangedFile>;
  totalFileCount: number;
  description: string;
};

export enum CommitCloudBackupStatus {
  InProgress = 'IN_PROGRESS',
  Pending = 'PENDING',
  Failed = 'FAILED',
}
export type CommitCloudSyncState = {
  isFetching?: boolean;
  /** Last time we ran commands to check the cloud status */
  lastChecked: Date;
  /** Last time there was an actual sync */
  lastBackup?: Date;
  currentWorkspace?: string;
  workspaceChoices?: Array<string>;
  commitStatuses?: Map<Hash, CommitCloudBackupStatus>;
  fetchError?: Error;
  syncError?: Error;
  workspaceError?: Error;
};

/**
 * A file can be auto-generated, partially auto-generated, or not generated (manual).
 * Numbered according to expected visual sort order.
 */
export enum GeneratedStatus {
  Manual = 0,
  PartiallyGenerated = 1,
  Generated = 2,
}

type ConflictInfo = {
  command: string;
  toContinue: string;
  toAbort: string;
  files: Array<ChangedFile>;
  fetchStartTimestamp: number;
  fetchCompletedTimestamp: number;
};
export type MergeConflicts =
  | ({state: 'loading'} & AllUndefined<ConflictInfo>)
  | ({
      state: 'loaded';
    } & ConflictInfo);

/* Operations */

export type RunnableOperation = {
  args: Array<CommandArg>;
  id: string;
  stdin?: string | undefined;
  runner: CommandRunner;
  trackEventName: TrackEventName;
};

export type OperationProgress =
  // another operation is running, so this one has been queued to run. Also include full state of the queue.
  | {id: string; kind: 'queue'; queue: Array<string>}
  // the server has started the process. This also servers as a "dequeue" notification. Also include full state of the queue.
  | {id: string; kind: 'spawn'; queue: Array<string>}
  | {id: string; kind: 'stderr'; message: string}
  | {id: string; kind: 'stdout'; message: string}
  // overally progress information, typically for a progress bar or progress not found directly in the stdout
  | {id: string; kind: 'progress'; progress: ProgressStep}
  // progress information for a specific commit, shown inline. Null hash means to apply the messasge to all hashes. Null message means to clear the message.
  | {id: string; kind: 'inlineProgress'; hash?: string; message?: string}
  | {id: string; kind: 'exit'; exitCode: number; timestamp: number}
  | {id: string; kind: 'error'; error: string};

export type ProgressStep = {
  message: string;
  progress?: number;
  progressTotal?: number;
};

export type OperationCommandProgressReporter = (
  ...args:
    | ['spawn']
    | ['stdout', string]
    | ['stderr', string]
    // null message -> clear inline progress for this hash. Null hash -> apply to all affected hashes (set message or clear)
    | [type: 'inlineProgress', hash?: string, message?: string]
    | ['progress', ProgressStep]
    | ['exit', number]
) => void;

export type OperationProgressEvent = {type: 'operationProgress'} & OperationProgress;

/** A line number starting from 1 */
export type OneIndexedLineNumber = Exclude<number, 0>;

/* protocol */

/**
 * messages sent by platform-specific (browser, vscode, electron) implementations
 * to be handled uniquely per server type.
 */
export type PlatformSpecificClientToServerMessages =
  | {type: 'platform/openFile'; path: RepoRelativePath; options?: {line?: OneIndexedLineNumber}}
  | {type: 'platform/openContainingFolder'; path: RepoRelativePath}
  | {type: 'platform/openDiff'; path: RepoRelativePath; comparison: Comparison}
  | {type: 'platform/openExternal'; url: string}
  | {type: 'platform/confirm'; message: string; details?: string | undefined}
  | {type: 'platform/subscribeToAvailableCwds'}
  | {
      type: 'platform/setVSCodeConfig';
      config: string;
      value: Json | undefined;
      scope: 'workspace' | 'global';
    }
  | {type: 'platform/executeVSCodeCommand'; command: string; args: Array<Json>}
  | {type: 'platform/gotUiState'; state: string}
  | {type: 'platform/subscribeToVSCodeConfig'; config: string};

/**
 * messages returned by platform-specific (browser, vscode, electron) server implementation,
 * usually in response to a platform-specific ClientToServer message
 */
export type PlatformSpecificServerToClientMessages =
  | {type: 'platform/getUiState'}
  | {
      type: 'platform/confirmResult';
      result: boolean;
    }
  | {
      type: 'platform/availableCwds';
      options: Array<AbsolutePath>;
    }
  | {
      type: 'platform/vscodeConfigChanged';
      config: string;
      value: Json | undefined;
    };

export type CodeReviewProviderSpecificClientToServerMessages =
  | never
  | InternalTypes['PhabricatorClientToServerMessages'];

export type PageVisibility = 'focused' | 'visible' | 'hidden';

export type FileABugFields = {title: string; description: string; repro: string};
export type FileABugProgress =
  | {status: 'starting'}
  | {status: 'inProgress'; message: string}
  | {status: 'success'; taskNumber: string; taskLink: string}
  | {status: 'error'; error: Error};
export type FileABugProgressMessage = {type: 'fileBugReportProgress'} & FileABugProgress;

/**
 * Like ClientToServerMessage, but these messages will be followed
 * on the message bus by an additional binary ArrayBuffer payload message.
 */
export type ClientToServerMessageWithPayload = {
  type: 'uploadFile';
  filename: string;
  id: string;
} & {hasBinaryPayload: true};

export type SubscriptionKind = 'uncommittedChanges' | 'smartlogCommits' | 'mergeConflicts';

/** sl configs written by ISL */
export type ConfigName =
  // these config names are for compatibility.
  | 'isl.submitAsDraft'
  | 'isl.changedFilesDisplayType'
  | 'isl.hasShownGettingStarted'
  // sapling config prefers foo-bar naming.
  | 'isl.pull-button-choice'
  | 'isl.show-stack-submit-confirmation'
  | 'isl.show-diff-number'
  | 'isl.render-compact'
  | 'isl.download-commit-should-goto'
  | 'isl.download-commit-rebase-type'
  | 'isl.experimental-features';

/** local storage keys written by ISL */
export type LocalStorageName =
  | 'isl.drawer-state'
  | 'isl.ui-zoom'
  | 'isl.has-shown-getting-started'
  | 'isl.amend-autorestack';

export type ClientToServerMessage =
  | {type: 'heartbeat'; id: string}
  | {type: 'refresh'}
  | {type: 'getConfig'; name: ConfigName}
  | {type: 'setConfig'; name: ConfigName; value: string}
  | {type: 'changeCwd'; cwd: string}
  | {type: 'track'; data: TrackDataWithEventName}
  | {type: 'fileBugReport'; data: FileABugFields; uiState?: Json}
  | {type: 'runOperation'; operation: RunnableOperation}
  | {type: 'abortRunningOperation'; operationId: string}
  | {type: 'fetchGeneratedStatuses'; paths: Array<RepoRelativePath>}
  | {type: 'fetchCommitMessageTemplate'}
  | {type: 'fetchShelvedChanges'}
  | {type: 'fetchLatestCommit'; revset: string}
  | {type: 'fetchAllCommitChangedFiles'; hash: Hash}
  | {type: 'renderMarkup'; markup: string; id: number}
  | {type: 'typeahead'; kind: TypeaheadKind; query: string; id: string}
  | {type: 'requestRepoInfo'}
  | {type: 'requestApplicationInfo'}
  | {type: 'fetchAvatars'; authors: Array<string>}
  | {type: 'fetchCommitCloudState'}
  | {type: 'fetchDiffSummaries'; diffIds?: Array<DiffId>}
  | {type: 'fetchLandInfo'; topOfStack: DiffId}
  | {type: 'confirmLand'; landConfirmationInfo: LandConfirmationInfo}
  | {type: 'getSuggestedReviewers'; context: {paths: Array<string>}; key: string}
  | {type: 'updateRemoteDiffMessage'; diffId: DiffId; title: string; description: string}
  | {type: 'pageVisibility'; state: PageVisibility}
  | {type: 'requestComparison'; comparison: Comparison}
  | {
      type: 'requestComparisonContextLines';
      id: {
        comparison: Comparison;
        path: RepoRelativePath;
      };
      start: number;
      numLines: number;
    }
  | {type: 'loadMoreCommits'}
  | {type: 'subscribe'; kind: SubscriptionKind; subscriptionID: string}
  | {type: 'unsubscribe'; kind: SubscriptionKind; subscriptionID: string}
  | {type: 'exportStack'; revs: string; assumeTracked?: Array<string>}
  | {type: 'importStack'; stack: ImportStack}
  | {type: 'fetchFeatureFlag'; name: string}
  | {type: 'fetchInternalUserInfo'}
  | {
      type: 'generateAICommitMessage';
      id: string;
      title: string;
      comparison: Comparison;
    }
  | CodeReviewProviderSpecificClientToServerMessages
  | PlatformSpecificClientToServerMessages;

export type SubscriptionResultsData = {
  uncommittedChanges: FetchedUncommittedChanges;
  smartlogCommits: FetchedCommits;
  mergeConflicts: MergeConflicts | undefined;
};

export type SubscriptionResult<K extends SubscriptionKind> = {
  type: 'subscriptionResult';
  subscriptionID: string;
  kind: K;
  data: SubscriptionResultsData[K];
};

export type ServerToClientMessage =
  | SubscriptionResult<'smartlogCommits'>
  | SubscriptionResult<'uncommittedChanges'>
  | SubscriptionResult<'mergeConflicts'>
  | BeganFetchingSmartlogCommitsEvent
  | BeganFetchingUncommittedChangesEvent
  | FileABugProgressMessage
  | {type: 'heartbeat'; id: string}
  | {type: 'gotConfig'; name: ConfigName; value: string | undefined}
  | {
      type: 'fetchedGeneratedStatuses';
      results: Record<RepoRelativePath, GeneratedStatus>;
    }
  | {type: 'fetchedCommitMessageTemplate'; template: string}
  | {type: 'fetchedShelvedChanges'; shelvedChanges: Result<Array<ShelvedChange>>}
  | {type: 'fetchedLatestCommit'; info: Result<CommitInfo>; revset: string}
  | {type: 'fetchedAllCommitChangedFiles'; hash: Hash; result: Result<Array<ChangedFile>>}
  | {type: 'typeaheadResult'; id: string; result: Array<TypeaheadResult>}
  | {type: 'applicationInfo'; info: ApplicationInfo}
  | {type: 'repoInfo'; info: RepoInfo; cwd?: string}
  | {type: 'repoError'; error: RepositoryError | undefined}
  | {type: 'fetchedAvatars'; avatars: Map<string, string>}
  | {type: 'fetchedDiffSummaries'; summaries: Result<Map<DiffId, DiffSummary>>}
  | {type: 'fetchedLandInfo'; topOfStack: DiffId; landInfo: Result<LandInfo>}
  | {type: 'confirmedLand'; result: Result<undefined>}
  | {type: 'fetchedCommitCloudState'; state: Result<CommitCloudSyncState>}
  | {type: 'renderedMarkup'; html: string; id: number}
  | {type: 'gotSuggestedReviewers'; reviewers: Array<string>; key: string}
  | {type: 'updatedRemoteDiffMessage'; diffId: DiffId; error?: string}
  | {type: 'uploadFileResult'; id: string; result: Result<string>}
  | {type: 'comparison'; comparison: Comparison; data: ComparisonData}
  | {type: 'comparisonContextLines'; path: RepoRelativePath; lines: Array<string>}
  | {type: 'beganLoadingMoreCommits'}
  | {type: 'commitsShownRange'; rangeInDays: number | undefined}
  | {type: 'additionalCommitAvailability'; moreAvailable: boolean}
  | {
      type: 'exportedStack';
      revs: string;
      assumeTracked: Array<string>;
      stack: ExportStack;
      error: string | undefined;
    }
  | {type: 'importedStack'; imported: ImportedStack; error: string | undefined}
  | {type: 'fetchedFeatureFlag'; name: string; passes: boolean}
  | {type: 'fetchedInternalUserInfo'; info: Serializable}
  | {
      type: 'generatedAICommitMessage';
      message: Result<string>;
      id: string;
    }
  | OperationProgressEvent
  | PlatformSpecificServerToClientMessages;

export type Disposable = {
  dispose(): void;
};

export type ComparisonData = {
  diff: Result<string>;
};
