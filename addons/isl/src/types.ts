/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TypeaheadResult} from 'isl-components/Types';
import type {TrackEventName} from 'isl-server/src/analytics/eventNames';
import type {TrackDataWithEventName} from 'isl-server/src/analytics/types';
import type {GitHubDiffSummary} from 'isl-server/src/github/githubCodeReviewProvider';
import type {Comparison} from 'shared/Comparison';
import type {ParsedDiff} from 'shared/patch/parse';
import type {AllUndefined, Json} from 'shared/typeUtils';
import type {Hash} from 'shared/types/common';
import type {ExportStack, ImportedStack, ImportStack} from 'shared/types/stack';
import type {TypeaheadKind} from './CommitInfoView/types';
import type {InternalTypes} from './InternalTypes';
import type {Serializable} from './serialize';
import type {Args, DiffCommit, PartiallySelectedDiffCommit} from './stackEdit/diffSplitTypes';

export type Result<T> = {value: T; error?: undefined} | {value?: undefined; error: Error};

/** known supported "Platforms" in which ISL may be embedded */
export type PlatformName =
  | 'browser'
  | 'androidStudio'
  | 'androidStudioRemote'
  | 'vscode'
  | 'webview'
  | 'chromelike_app'
  | 'visualStudio';

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

export type DiffCommentReaction = {
  name: string;
  reaction:
    | 'ANGER'
    | 'HAHA'
    | 'LIKE'
    | 'LOVE'
    | 'WOW'
    | 'SORRY'
    | 'SAD'
    | 'CONFUSED'
    | 'EYES'
    | 'HEART'
    | 'HOORAY'
    | 'LAUGH'
    | 'ROCKET'
    | 'THUMBS_DOWN'
    | 'THUMBS_UP';
};

export type InternalCommitMessageFields = InternalTypes['InternalCommitMessageFields'];

export enum CodePatchSuggestionStatus {
  Accepted = 'ACCEPTED',
  Declined = 'DECLINED',
  Unset = 'UNSET',
}

export enum SuggestedChangeType {
  HUMAN_SUGGESTION = 'HUMAN_SUGGESTION',
  METAMATE_SUGGESTION = 'METAMATE_SUGGESTION',
  CI_SIGNAL = 'CI_SIGNAL',
}

export enum ArchivedStateType {
  ARCHIVED = 'ARCHIVED',
  NOT_ARCHIVED = 'NOT_ARCHIVED',
}

export enum ArchivedReasonType {
  APPLIED_IN_EDITOR = 'APPLIED_IN_EDITOR',
  APPLIED_MERGED = 'APPLIED_MERGED',
  APPLIED_STACKED_DIFF = 'APPLIED_STACKED_DIFF',
  AUTHOR_DISCARDED = 'AUTHOR_DISCARDED',
  STALE_DIFF_CLOSED = 'STALE_DIFF_CLOSED',
  STALE_FILE_CHANGED = 'STALE_FILE_CHANGED',
}

export type CodeChange = {
  oldContent?: string;
  newContent?: string;
  oldLineNumber?: number;
  trimmedLineNumber?: number;
  trimmedLength?: number;
  adjustedLineNumber?: number;
  patch?: ParsedDiff;
};

export type SuggestedChange = {
  id?: string;
  type?: SuggestedChangeType;
  codePatchSuggestionID?: string;
  codePatchID?: string;
  status?: CodePatchSuggestionStatus;
  archivedState?: ArchivedStateType;
  archivedReason?: ArchivedReasonType;
  commitHashBefore?: string;
  patch?: ParsedDiff;
  oldPath?: string;
  currentPath?: string;
  codeChange?: CodeChange[];
};

export type DiffComment = {
  id?: string;
  author: string;
  authorName?: string;
  authorAvatarUri?: string;
  html: string;
  content?: string;
  created: Date;
  commitHash?: string;
  /** If it's an inline comment, this is the file path with the comment */
  filename?: string;
  /** If it's an inline comment, this is the line it was added */
  line?: number;
  reactions: Array<DiffCommentReaction>;
  /** Suggestion for how to change the code, as a patch */
  suggestedChange?: SuggestedChange;
  replies: Array<DiffComment>;
  /** If this comment has been resolved. true => "resolved", false => "unresolved", null => the comment is not resolvable, don't show any UI for it */
  isResolved?: boolean;
};

/**
 * Summary of CI test results for a Diff.
 * 'pass' if ALL signals succeed and not still running.
 * 'failed' if ANY signal doesn't succeed, even if some are still running.
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
      path: string | undefined;
    }
  | {type: 'edenFsUnhealthy'; cwd: string}
  | {type: 'cwdNotARepository'; cwd: string}
  | {type: 'cwdDoesNotExist'; cwd: string}
  | {
      type: 'unknownError';
      error: Error;
    };

export type CwdInfo = {
  /** Full cwd path, like /Users/username/repoRoot/some/subfolder */
  cwd: AbsolutePath;
  /** Full real path to the repository root, like /Users/username/repoRoot
   * Undefined for cwds that are not valid repositories */
  repoRoot?: AbsolutePath;
  /** Label for a cwd, which is <repoBasename>/<cwd>, like 'sapling/addons'.
   * Intended for display. Undefined for cwds that are not valid repositories */
  repoRelativeCwdLabel?: string;
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

/**
 * Which "mode" for the App to run. Controls the basic rendering.
 * Useful to render full-screen alternate views.
 * isl => normal, full ISL
 * comparison => just the comparison viewer is rendered, set to some specific comparison
 */
export type AppMode =
  | {
      mode: 'isl';
    }
  | {
      mode: 'comparison';
      comparison: Comparison;
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
      callsign?: string;
    }
  | {
      type: 'none';
    }
  | {
      type: 'unknown';
      path?: string;
    };

export type PreferredSubmitCommand = 'pr' | 'ghstack' | 'push';

export type StableCommitMetadata = {
  value: string;
  description: string;
};

export type StableCommitFetchConfig = {
  template: string;
  parse: (data: string) => Array<StableCommitMetadata>;
};

export type StableLocationData = {
  /** Stables found automatically from recent builds */
  stables: Array<Result<StableInfo>>;
  /** Stables that enabled automatically for certain users */
  special: Array<Result<StableInfo>>;
  /** Stables entered in the UI. Map of provided name to a Result. null means the stable is loading. */
  manual: Record<string, Result<StableInfo> | null>;
  /** Whether this repo supports entering custom stables via input. */
  repoSupportsCustomStables: boolean;
};
export type StableInfo = {
  hash: string;
  name: string;
  /** If present, this is informational text, like the reason it's been added */
  byline?: string;
  /** If present, this is extra details that might be shown in a tooltip */
  info?: string;
  date: Date;
};

export type SlocInfo = {
  /** Significant lines of code for commit */
  sloc: number | undefined;
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
  parents: ReadonlyArray<Hash>;
  /**
   * Grandparents are the closest but indirect ancestors in a set of commits .
   * In ISL, this is used for connecting nodes whose direct parents are NOT present.
   * Note that this field will be empty by design when direct parents are already present in the set.
   * See eden/scm/tests/test-template-grandparents.t for examples.
   */
  grandparents: ReadonlyArray<Hash>;
  phase: CommitPhaseType;
  /**
   * Whether this commit is the "." (working directory parent).
   * It is the parent of "wdir()" or the "You are here" virtual commit.
   */
  isDot: boolean;
  author: string;
  date: Date;
  description: string;
  bookmarks: ReadonlyArray<string>;
  remoteBookmarks: ReadonlyArray<string>;
  /** if this commit is obsolete, it is succeeded by another commit */
  successorInfo?: Readonly<SuccessorInfo>;
  /**
   * Closest predecessors (not all recursive predecessors, which can be a long
   * chain and hurt performance). Useful to deal with optimistic states where
   * we know the hashes of predecessors (commits being rewritten) but not their
   * successors (rewritten result).
   *
   * Most of the time a commit only has one predecessor. In case of a fold
   * there are multiple predecessors.
   */
  closestPredecessors?: ReadonlyArray<Hash>;
  /**
   * If this is a fake optimistic commit created by a running Operation,
   * this is the revset that can be used by sl to find the real commit.
   * This is only valid after the operation which creates this commit has completed.
   */
  optimisticRevset?: Revset;
  /** only a subset of the total changed file paths for this commit.
   * File statuses must be fetched separately for performance.
   */
  filePathsSample: ReadonlyArray<RepoRelativePath>;
  totalFileCount: number;
  /** @see {@link DiffId} */
  diffId?: DiffId;
  isFollower?: boolean;
  stableCommitMetadata?: ReadonlyArray<StableCommitMetadata>;
  /**
   * Longest path prefix shared by all files in this commit.
   * For example, if a commit changes files like `a/b/c` and `a/b/d`, this is `a/b/`.
   * Note: this always acts on `/` delimited paths, and is done on complete subdir names,
   * never on matching prefixes of directories. For example, `a/dir1/a` and `a/dir2/a`
   * have `a/` as the common prefix, not `a/dir`.
   * If no commonality is found (due to edits to top level files or multiple subdirs), this is empty string.
   * This can be useful to determine if a commit is relevant to your cwd.
   */
  maxCommonPathPrefix: RepoRelativePath;
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
export type FilesSample = {
  filesSample: Array<ChangedFile>;
  totalFileCount: number;
};

/** A revset that selects for a commit which we only have a fake optimistic preview of */
export type OptimisticRevset = {type: 'optimistic-revset'; revset: Revset; fake: string};
/** A revset that selects for the latest version of a commit hash */
export type SucceedableRevset = {type: 'succeedable-revset'; revset: Revset};
/** A revset that selects for a specific commit, without considering any successors */
export type ExactRevset = {type: 'exact-revset'; revset: Revset};

/**
 * Most arguments to eden commands are literal `string`s, except:
 * - When specifying file paths, the server needs to know which args are files to convert them to be cwd-relative.
 *     - For long file lists, we pass them in a single bulk arg, which will be passed via stdin instead
 *       to avoid command line length limits.
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
  | {type: 'repo-relative-file-list'; paths: Array<RepoRelativePath>}
  | {type: 'config'; key: string; value: string}
  | ExactRevset
  | SucceedableRevset
  | OptimisticRevset;

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
 * {@link CommandArg} representing a hash or revset for a fake optimistic commit.
 * This enables queued commands to act on optimistic state without knowing
 * the optimistic commit's hashes directly, and without knowing a predecessor hash at all.
 * The fake optimistic commit hash is also stored to know what the revset refers to.
 */
export function optimisticRevset(revset: Revset, fake: string): OptimisticRevset {
  return {type: 'optimistic-revset', revset, fake};
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

/** Responses to subscriptions, including data and the time duration the fetch lasted */
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
  // if true, commit cloud is disabled in this repo
  isDisabled?: boolean;
};

export type AlertSeverity = 'SEV 0' | 'SEV 1' | 'SEV 2' | 'SEV 3' | 'SEV 4' | 'UBN';
export type Alert = {
  key: string;
  title: string;
  description: string;
  url: string;
  severity: AlertSeverity;
  ['show-in-isl']: boolean;
  ['isl-version-regex']?: string;
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

export enum ConflictType {
  BothChanged = 'both_changed',
  DeletedInDest = 'dest_deleted',
  DeletedInSource = 'source_deleted',
}

type ConflictInfo = {
  command: string;
  toContinue: string;
  toAbort: string;
  files: Array<ChangedFile & {conflictType: ConflictType}>;
  fetchStartTimestamp: number;
  fetchCompletedTimestamp: number;
  hashes?: {local?: string; other?: string};
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
  | {id: string; kind: 'error'; error: string}
  | {id: string; kind: 'warning'; warning: string}
  // used by requestMissedOperationProgress, client thinks this operation is running but server no longer knows about it.
  | {id: string; kind: 'forgot'};

export type ProgressStep = {
  message: string;
  progress?: number;
  progressTotal?: number;
  unit?: string;
};

export type OperationCommandProgressReporter = (
  ...args:
    | ['spawn']
    | ['stdout', string]
    | ['stderr', string]
    // null message -> clear inline progress for this hash. Null hash -> apply to all affected hashes (set message or clear)
    | [type: 'inlineProgress', hash?: string, message?: string]
    | ['progress', ProgressStep]
    | ['warning', string]
    | ['exit', number]
) => void;

export type OperationProgressEvent = {type: 'operationProgress'} & OperationProgress;

/** A line number starting from 1 */
export type OneIndexedLineNumber = Exclude<number, 0>;

export type DiagnosticSeverity = 'error' | 'warning' | 'info' | 'hint';

export type Diagnostic = {
  range: {startLine: number; startCol: number; endLine: number; endCol: number};
  message: string;
  severity: DiagnosticSeverity;
  /** LSP providing this diagnostic, like "typescript" or "eslint" */
  source?: string;
  /** Code or name for this kind of diagnostic */
  code?: string;
};

export type DiagnosticAllowlistValue =
  | {block: Set<string>; allow?: undefined}
  | {allow: Set<string>; block?: undefined};
export type DiagnosticAllowlist = Map<'warning' | 'error', Map<string, DiagnosticAllowlistValue>>;

/* protocol */

/**
 * messages sent by platform-specific (browser, vscode, electron) implementations
 * to be handled uniquely per server type.
 */
export type PlatformSpecificClientToServerMessages =
  | {type: 'platform/openFile'; path: RepoRelativePath; options?: {line?: OneIndexedLineNumber}}
  | {
      type: 'platform/openFiles';
      paths: ReadonlyArray<RepoRelativePath>;
      options?: {line?: OneIndexedLineNumber};
    }
  | {type: 'platform/openContainingFolder'; path: RepoRelativePath}
  | {type: 'platform/openDiff'; path: RepoRelativePath; comparison: Comparison}
  | {type: 'platform/openExternal'; url: string}
  | {type: 'platform/changeTitle'; title: string}
  | {type: 'platform/confirm'; message: string; details?: string | undefined}
  | {type: 'platform/subscribeToAvailableCwds'}
  | {type: 'platform/subscribeToUnsavedFiles'}
  | {type: 'platform/saveAllUnsavedFiles'}
  | {type: 'platform/setPersistedState'; key: string; data?: string}
  | {
      type: 'platform/setVSCodeConfig';
      config: string;
      value: Json | undefined;
      scope: 'workspace' | 'global';
    }
  | {type: 'platform/checkForDiagnostics'; paths: Array<RepoRelativePath>}
  | {type: 'platform/executeVSCodeCommand'; command: string; args: Array<Json>}
  | {type: 'platform/subscribeToVSCodeConfig'; config: string};

/**
 * messages returned by platform-specific (browser, vscode, electron) server implementation,
 * usually in response to a platform-specific ClientToServer message
 */
export type PlatformSpecificServerToClientMessages =
  | {
      type: 'platform/confirmResult';
      result: boolean;
    }
  | {
      type: 'platform/availableCwds';
      options: Array<CwdInfo>;
    }
  | {type: 'platform/unsavedFiles'; unsaved: Array<{path: RepoRelativePath; uri: string}>}
  | {type: 'platform/savedAllUnsavedFiles'; success: boolean}
  | {
      type: 'platform/gotDiagnostics';
      diagnostics: Map<RepoRelativePath, Array<Diagnostic>>;
    }
  | {
      type: 'platform/vscodeConfigChanged';
      config: string;
      value: Json | undefined;
    };

export type CodeReviewProviderSpecificClientToServerMessages =
  | never
  | InternalTypes['PhabricatorClientToServerMessages'];

export type CodeReviewProviderSpecificServerToClientMessages =
  | never
  | InternalTypes['PhabricatorServerToClientMessages'];

export type PageVisibility = 'focused' | 'visible' | 'hidden';

export type FileABugFields = {title: string; description: string; repro: string};
export type FileABugProgress =
  | {status: 'starting'}
  | {
      status: 'inProgress';
      currentSteps: Record<string, 'blocked' | 'loading' | 'finished'>;
    }
  | {status: 'success'; taskNumber: string; taskLink: string}
  | {status: 'error'; error: Error};
export type FileABugProgressMessage = {type: 'fileBugReportProgress'} & FileABugProgress;

export type SubscriptionKind = 'uncommittedChanges' | 'smartlogCommits' | 'mergeConflicts';

export const allConfigNames = [
  // these config names are for compatibility.
  'isl.submitAsDraft',
  'isl.changedFilesDisplayType',
  // sapling config prefers foo-bar naming.
  'isl.pull-button-choice',
  'isl.show-stack-submit-confirmation',
  'isl.show-diff-number',
  'isl.render-compact',
  'isl.download-commit-should-goto',
  'isl.download-commit-rebase-type',
  'isl.experimental-features',
  'isl.hold-off-refresh-ms',
  'isl.sl-progress-enabled',
  'isl.use-sl-graphql',
  'github.preferred_submit_command',
  'isl.open-file-cmd',
  'isl.generated-files-regex',
  'ui.username',
  'ui.merge',
  'fbcodereview.code-browser-url',
  'extensions.commitcloud',
] as const;

/** sl configs read by ISL */
export type ConfigName = (typeof allConfigNames)[number];

/**
 * Not all configs should be set-able from the UI, for security.
 * Only configs which could not possibly allow code execution should be allowed.
 * This also includes values allowed to be passed in the args for Operations.
 * Most ISL configs are OK.
 */
export const settableConfigNames = [
  'isl.submitAsDraft',
  'isl.changedFilesDisplayType',
  'isl.pull-button-choice',
  'isl.show-stack-submit-confirmation',
  'isl.show-diff-number',
  'isl.render-compact',
  'isl.download-commit-should-goto',
  'isl.download-commit-rebase-type',
  'isl.experimental-features',
  'isl.hold-off-refresh-ms',
  'isl.use-sl-graphql',
  'isl.experimental-graph-renderer',
  'isl.generated-files-regex',
  'github.preferred_submit_command',
  'ui.allowemptycommit',
  'ui.merge',
  'amend.autorestack',
] as const;

/** sl configs written to by ISL */
export type SettableConfigName = (typeof settableConfigNames)[number];

/** local storage keys written by ISL */
export type LocalStorageName =
  | 'isl.drawer-state'
  | 'isl.bookmarks'
  | 'isl.ui-zoom'
  | 'isl.has-shown-getting-started'
  | 'isl.dismissed-split-suggestion'
  | 'isl.amend-autorestack'
  | 'isl.dismissed-alerts'
  | 'isl.debug-react-tools'
  | 'isl.debug-redux-tools'
  | 'isl.condense-obsolete-stacks'
  | 'isl.deemphasize-cwd-irrelevant-commits'
  | 'isl.hide-cwd-irrelevant-stacks'
  | 'isl.split-suggestion-enabled'
  | 'isl.comparison-display-mode'
  | 'isl.expand-generated-files'
  | 'isl-color-theme'
  | 'isl.auto-resolve-before-continue'
  | 'isl.warn-about-diagnostics'
  | 'isl.hide-non-blocking-diagnostics'
  | 'isl.rebase-off-warm-warning-enabled'
  | 'isl.experimental-features-local-override'
  // These keys are prefixes, with further dynamic keys appended afterwards
  | 'isl.edited-commit-messages:'
  | 'isl.partial-abort';

export type ClientToServerMessage =
  | {type: 'heartbeat'; id: string}
  | {type: 'stress'; id: number; time: number; message: string}
  | {type: 'refresh'}
  | {type: 'getConfig'; name: ConfigName}
  | {type: 'setConfig'; name: SettableConfigName; value: string}
  | {type: 'setDebugLogging'; name: 'debug' | 'verbose'; enabled: boolean}
  | {type: 'changeCwd'; cwd: string}
  | {type: 'track'; data: TrackDataWithEventName}
  | {type: 'fileBugReport'; data: FileABugFields; uiState?: Json; collectRage: boolean}
  | {type: 'runOperation'; operation: RunnableOperation}
  | {type: 'abortRunningOperation'; operationId: string}
  | {type: 'fetchActiveAlerts'}
  | {type: 'fetchGeneratedStatuses'; paths: Array<RepoRelativePath>}
  | {type: 'fetchCommitMessageTemplate'}
  | {type: 'fetchShelvedChanges'}
  | {type: 'fetchLatestCommit'; revset: string}
  | {type: 'fetchCommitChangedFiles'; hash: Hash; limit?: number}
  | {
      type: 'uploadFile';
      filename: string;
      id: string;
      b64Content: string;
    }
  | {type: 'renderMarkup'; markup: string; id: number}
  | {type: 'typeahead'; kind: TypeaheadKind; query: string; id: string}
  | {type: 'requestRepoInfo'}
  | {type: 'requestApplicationInfo'}
  | {type: 'requestMissedOperationProgress'; operationId: string}
  | {type: 'fetchAvatars'; authors: Array<string>}
  | {type: 'fetchCommitCloudState'}
  | {type: 'fetchDiffSummaries'; diffIds?: Array<DiffId>}
  | {type: 'fetchDiffComments'; diffId: DiffId}
  | {type: 'fetchLandInfo'; topOfStack: DiffId}
  | {type: 'fetchAndSetStables'; additionalStables: Array<string>}
  | {type: 'fetchStableLocationAutocompleteOptions'}
  | {type: 'confirmLand'; landConfirmationInfo: LandConfirmationInfo}
  | {type: 'getSuggestedReviewers'; context: {paths: Array<string>}; key: string}
  | {type: 'getConfiguredMergeTool'}
  | {type: 'updateRemoteDiffMessage'; diffId: DiffId; title: string; description: string}
  | {type: 'pageVisibility'; state: PageVisibility}
  | {type: 'getRepoUrlAtHash'; revset: Revset; path?: string}
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
  | {type: 'fetchQeFlag'; name: string}
  | {type: 'fetchFeatureFlag'; name: string}
  | {type: 'fetchInternalUserInfo'}
  | {
      type: 'generateSuggestionWithAI';
      id: string;
      comparison: Comparison;
      fieldName: string;
      latestFields: InternalCommitMessageFields;
      suggestionId: string;
    }
  | {type: 'splitCommitWithAI'; id: string; diffCommit: DiffCommit; args: Args}
  | {type: 'gotUiState'; state: string}
  | CodeReviewProviderSpecificClientToServerMessages
  | PlatformSpecificClientToServerMessages
  | {type: 'fetchSignificantLinesOfCode'; hash: Hash; excludedFiles: string[]}
  | {
      type: 'fetchPendingSignificantLinesOfCode';
      requestId: number;
      hash: Hash;
      includedFiles: string[];
    }
  | {
      type: 'fetchPendingAmendSignificantLinesOfCode';
      requestId: number;
      hash: Hash;
      includedFiles: string[];
    }
  | {
      type: 'fetchGkDetails';
      id: string;
      name: string;
    }
  | {
      type: 'fetchJkDetails';
      id: string;
      names: string[];
    }
  | {
      type: 'fetchKnobsetDetails';
      id: string;
      configPath: string;
    }
  | {
      type: 'fetchQeDetails';
      id: string;
      name: string;
    };

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
  | {type: 'stress'; id: number; time: number; message: string}
  | {type: 'gotConfig'; name: ConfigName; value: string | undefined}
  | {
      type: 'fetchedGeneratedStatuses';
      results: Record<RepoRelativePath, GeneratedStatus>;
    }
  | {type: 'fetchedActiveAlerts'; alerts: Array<Alert>}
  | {type: 'fetchedCommitMessageTemplate'; template: string}
  | {type: 'fetchedShelvedChanges'; shelvedChanges: Result<Array<ShelvedChange>>}
  | {type: 'fetchedLatestCommit'; info: Result<CommitInfo>; revset: string}
  | {
      type: 'fetchedCommitChangedFiles';
      hash: Hash;
      result: Result<FilesSample>;
    }
  | {type: 'typeaheadResult'; id: string; result: Array<TypeaheadResult>}
  | {type: 'applicationInfo'; info: ApplicationInfo}
  | {type: 'repoInfo'; info: RepoInfo; cwd?: string}
  | {type: 'repoError'; error: RepositoryError | undefined}
  | {type: 'fetchedAvatars'; avatars: Map<string, string>; authors: Array<string>}
  | {type: 'fetchedDiffSummaries'; summaries: Result<Map<DiffId, DiffSummary>>}
  | {type: 'fetchedDiffComments'; diffId: DiffId; comments: Result<Array<DiffComment>>}
  | {type: 'fetchedLandInfo'; topOfStack: DiffId; landInfo: Result<LandInfo>}
  | {type: 'confirmedLand'; result: Result<undefined>}
  | {type: 'fetchedCommitCloudState'; state: Result<CommitCloudSyncState>}
  | {type: 'fetchedStables'; stables: StableLocationData}
  | {type: 'fetchedStableLocationAutocompleteOptions'; result: Result<Array<TypeaheadResult>>}
  | {type: 'renderedMarkup'; html: string; id: number}
  | {type: 'gotSuggestedReviewers'; reviewers: Array<string>; key: string}
  | {type: 'gotConfiguredMergeTool'; tool: string | undefined}
  | {type: 'updatedRemoteDiffMessage'; diffId: DiffId; error?: string}
  | {type: 'uploadFileResult'; id: string; result: Result<string>}
  | {type: 'gotRepoUrlAtHash'; url: Result<string>}
  | {type: 'comparison'; comparison: Comparison; data: ComparisonData}
  | {type: 'comparisonContextLines'; path: RepoRelativePath; lines: Result<Array<string>>}
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
  | {type: 'fetchedQeFlag'; name: string; passes: boolean}
  | {type: 'fetchedFeatureFlag'; name: string; passes: boolean}
  | {type: 'fetchedInternalUserInfo'; info: Serializable}
  | {
      type: 'generatedSuggestionWithAI';
      message: Result<string>;
      id: string;
    }
  | {
      type: 'splitCommitWithAI';
      id: string;
      result: Result<ReadonlyArray<PartiallySelectedDiffCommit>>;
    }
  | {type: 'getUiState'}
  | OperationProgressEvent
  | PlatformSpecificServerToClientMessages
  | CodeReviewProviderSpecificServerToClientMessages
  | {
      type: 'fetchedSignificantLinesOfCode';
      hash: Hash;
      result: Result<number>;
    }
  | {
      type: 'fetchedPendingSignificantLinesOfCode';
      requestId: number;
      hash: Hash;
      result: Result<number>;
    }
  | {
      type: 'fetchedPendingAmendSignificantLinesOfCode';
      requestId: number;
      hash: Hash;
      result: Result<number>;
    }
  | {
      type: 'fetchedGkDetails';
      id: string;
      result: Result<InternalTypes['InternalGatekeeper']>;
    }
  | {
      type: 'fetchedJkDetails';
      id: string;
      result: Result<InternalTypes['InternalJustknob']>;
    }
  | {
      type: 'fetchedKnobsetDetails';
      id: string;
      result: Result<InternalTypes['InternalKnobset']>;
    }
  | {
      type: 'fetchedQeDetails';
      id: string;
      result: Result<InternalTypes['InternalQuickExperiment']>;
    };

export type Disposable = {
  dispose(): void;
};

export type ComparisonData = {
  diff: Result<string>;
};

export type MessageBusStatus =
  | {type: 'initializing'}
  | {type: 'open'}
  | {type: 'reconnecting'}
  | {type: 'error'; error?: string};

export type ArcStableGKInfo = {
  gk: string;
  id: string;
  label: string;
};
