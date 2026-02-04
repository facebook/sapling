/**
 * This software contains information and intellectual property that is
 * confidential and proprietary to Facebook, Inc. and its affiliates.
 *
 * @generated
 */

/*
 * This file is synced between fbcode/eden/fs/facebook/prototypes/node-edenfs-notifications-client/index.d.ts.
 * The authoritative copy is the one in eden/fs/.
 * Use `yarn sync-edenfs-notifications` to perform the sync.
 *
 * This file is intended to be self contained so it may be copied/referenced from other extensions,
 * which is why it should not import anything and why it reimplements many types.
 */

/**
 * TypeScript type definitions for EdenFS Notifications Client
 * JavaScript interface for EdenFS CLI notify endpoint
 */

import {EventEmitter} from 'events';

/**
 * Options for initializing EdenFSNotificationsClient
 */
export interface EdenFSClientOptions {
  /** Path to the mount point */
  mountPoint?: string;
  /** Timeout in milliseconds for commands (default: 30000) */
  timeout?: number;
  /** Path to the eden binary (default: 'eden') */
  edenBinaryPath?: string;
}

/**
 * Journal position information
 */
export interface JournalPosition {
  journalPosition: string;
}

/**
 * Options for getStatus command
 */
export interface GetStatusOptions {
  /** Use case identifier for the command */
  useCase?: string;
}

/**
 * Options for waitReady command
 */
export interface WaitReadyOptions {
  /** Use case identifier for the command */
  useCase?: string;
  /** Timeout in milliseconds for the command (overrides constructor value) */
  timeout?: number;
}

/**
 * Options for getPosition command
 */
export interface GetPositionOptions {
  /** Use case identifier for the command */
  useCase?: string;
  /** Mount point path (overrides constructor value) */
  mountPoint?: string;
}

/**
 * Options for getChangesSince command
 */
export interface GetChangesSinceOptions {
  /** Journal position to start from */
  position?: string | JournalPosition;
  /** Use case identifier */
  useCase?: string;
  /** Mount point path (overrides constructor value) */
  mountPoint?: string;
  /** Relative root to scope results */
  relativeRoot?: string;
  /** Include VCS roots in output */
  includeVcsRoots?: boolean;
  /** Included roots in output */
  includedRoots?: string[];
  /** Excluded roots in output */
  excludedRoots?: string[];
  /** Included file suffixes in output */
  includedSuffixes?: string[];
  /** Excluded file suffixes in output */
  excludedSuffixes?: string[];
  /** Return JSON format (default: true) */
  json?: boolean;
  /** States to wait for deassertion */
  deferredStates?: string[];
}

/**
 * Options for subscription
 */
export interface SubscriptionOptions {
  /** Journal position to start from */
  position?: string;
  /** Use case identifier */
  useCase?: string;
  /** Mount point path */
  mountPoint?: string;
  /** Throttle in milliseconds between events (default: 0) */
  throttle?: number;
  /** Relative root to scope results */
  relativeRoot?: string;
  /** Include VCS roots in output */
  includeVcsRoots?: boolean;
  /** Included roots in output */
  includedRoots?: string[];
  /** Excluded roots in output */
  excludedRoots?: string[];
  /** Included file suffixes in output */
  includedSuffixes?: string[];
  /** Excluded file suffixes in output */
  excludedSuffixes?: string[];
  /** States to wait for deassertion */
  deferredStates?: string[];
  /** Path to eden binary */
  edenBinaryPath?: string;
}

/**
 * Options for enterState command
 */
export interface EnterStateOptions {
  /** Duration in seconds to maintain state */
  duration?: number;
  /** Use case identifier */
  useCase?: string;
  /** Mount point path (overrides constructor value) */
  mountPoint?: string;
}

/**
 * Small change types
 */
export interface AddedChange {
  path: number[];
  file_type: string;
}

export interface ModifiedChange {
  path: number[];
  file_type: string;
}

export interface RemovedChange {
  path: number[];
  file_type: string;
}

export interface RenamedChange {
  from: number[];
  to: number[];
  file_type: string;
}

export interface ReplacedChange {
  from: number[];
  to: number[];
  file_type: string;
}

export interface SmallChange {
  Added?: AddedChange;
  Modified?: ModifiedChange;
  Removed?: RemovedChange;
  Renamed?: RenamedChange;
  Replaced?: ReplacedChange;
}

/**
 * Large change types
 */
export interface DirectoryRenamedChange {
  from: number[];
  to: number[];
}

export interface CommitTransitionChange {
  from: number[];
  to: number[];
}

export interface LostChanges {
  reason: string;
}

export interface LargeChange {
  DirectoryRenamed?: DirectoryRenamedChange;
  CommitTransition?: CommitTransitionChange;
  LostChanges?: LostChanges;
}

/**
 * State change types
 */
export interface StateEnteredChange {
  state: string;
}

export interface StateLeftChange {
  state: string;
}

export interface StateChange {
  StateEntered?: StateEnteredChange;
  StateLeft?: StateLeftChange;
}

/**
 * File system change event
 */
export interface Change {
  SmallChange?: SmallChange;
  LargeChange?: LargeChange;
  StateChange?: StateChange;
}

/**
 * Response from getChangesSince
 */
export interface ChangesSinceResponse {
  /** List of changes */
  changes: Change[];
  /** New journal position after changes */
  to_position?: string;
}

/**
 * Event emitted by subscription
 */
export interface SubscriptionEvent extends ChangesSinceResponse {
  /** Position a state change occurred at */
  position?: string;
  /** Event type for state changes */
  event_type?: 'Entered' | 'Left';
  /** State name for state change events */
  state?: string;
}

/**
 * Callback for subscription events
 */
export type SubscriptionCallback = (error: Error | null, result: SubscriptionEvent | null) => void;

/**
 * Custom error class for EdenFS errors
 */
export class EdenFSError extends Error {
  edenFSResponse: any;
}

/**
 * EdenFS Notifications Client
 * Provides methods to interact with EdenFS notifications via the EdenFS CLI
 */
export class EdenFSNotificationsClient extends EventEmitter {
  mountPoint: string | null;
  timeout: number;
  edenBinaryPath: string;
  readonly DEFAULT_EDENFS_RECONNECT_DELAY_MS: number;
  readonly MAXIMUM_EDENFS_RECONNECT_DELAY_MS: number;

  constructor(options?: EdenFSClientOptions);

  /**
   * Get the current EdenFS status
   */
  getStatus(options?: GetStatusOptions): Promise<string>;

  /**
   * Wait until EdenFS is ready, using getStatus
   * @returns Promise that resolves to true if healthy, false if timeout
   */
  waitReady(options?: WaitReadyOptions): Promise<boolean>;

  /**
   * Get the current EdenFS journal position
   */
  getPosition(options?: GetPositionOptions): Promise<string>;

  /**
   * Get changes since a specific journal position
   */
  getChangesSince(options?: GetChangesSinceOptions): Promise<ChangesSinceResponse>;

  /**
   * Subscribe to filesystem changes
   */
  subscribe(options?: SubscriptionOptions, callback?: SubscriptionCallback): EdenFSSubscription;

  /**
   * Enter a specific state
   */
  enterState(state: string, options?: EnterStateOptions): Promise<void>;
}

/**
 * EdenFS Subscription
 * Handles real-time filesystem change notifications
 */
export class EdenFSSubscription extends EventEmitter {
  client: EdenFSNotificationsClient;
  options: SubscriptionOptions;
  process: any;
  edenBinaryPath: string;
  errData: string;
  killTimeout: NodeJS.Timeout | null;

  constructor(client: EdenFSNotificationsClient, options?: SubscriptionOptions);

  /**
   * Start the subscription
   */
  start(): Promise<void>;

  /**
   * Stop the subscription
   */
  stop(): void;

  // EventEmitter events
  on(event: 'change', listener: (event: SubscriptionEvent) => void): this;
  on(event: 'error', listener: (error: Error) => void): this;
  on(event: 'close', listener: () => void): this;

  once(event: 'change', listener: (event: SubscriptionEvent) => void): this;
  once(event: 'error', listener: (error: Error) => void): this;
  once(event: 'close', listener: () => void): this;

  emit(event: 'change', data: SubscriptionEvent): boolean;
  emit(event: 'error', error: Error): boolean;
  emit(event: 'close'): boolean;
}

/**
 * Utility functions for working with EdenFS notify data
 */
export class EdenFSUtils {
  /**
   * Convert byte array path to string
   */
  static bytesToPath(pathBytes: number[]): string;

  /**
   * Convert byte array to hex string
   */
  static bytesToHex(bytes: number[]): string;

  /**
   * Extract file type from change
   */
  static extractFileType(smallChange: SmallChange): string;

  /**
   * Extract file path(s) from single change
   */
  static extractPath(changes: SmallChange): string[];

  /**
   * Extract file paths from changes
   */
  static extractPaths(changes: Change[]): string[];

  /**
   * Get change type from a change object
   */
  static getChangeType(
    change: Change,
  ):
    | 'added'
    | 'modified'
    | 'removed'
    | 'renamed'
    | 'replaced'
    | 'directory renamed'
    | 'commit transition'
    | 'lost changes'
    | 'state entered'
    | 'state left'
    | 'unknown';
}

declare const exports: {
  EdenFSNotificationsClient: typeof EdenFSNotificationsClient;
  EdenFSSubscription: typeof EdenFSSubscription;
  EdenFSUtils: typeof EdenFSUtils;
};

export default exports;

export type Options = {
  mountPoint?: string;
  timeout?: number;
  edenBinaryPath?: string;
};

export type CommandCallback = (error?: Error, result?: Response) => void;
