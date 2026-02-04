/**
 * This software contains information and intellectual property that is
 * confidential and proprietary to Facebook, Inc. and its affiliates.
 *
 * @generated
 */

/* eslint-disable */

/*
 * This file is synced between fbcode/eden/fs/facebook/prototypes/node-edenfs-notifications-client/index.js.
 * The authoritative copy is the one in eden/fs/.
 * Use `yarn sync-edenfs-notifications` to perform the sync.
 *
 * This file is intended to be self contained so it may be copied/referenced from other extensions,
 * which is why it should not import anything and why it reimplements many types.
 */

/**
 * JavaScript interface for EdenFS CLI notify endpoint
 *
 * This module provides a JavaScript wrapper around the EdenFS CLI notify commands,
 * allowing you to monitor filesystem changes in EdenFS mounts.
 *
 * @author cqd
 * @version 1.0.0
 *
 * @format
 * @flow
 * @ts-check
 */

const {spawn, execFile} = require('child_process');
const {EventEmitter} = require('events');
const path = require('path');

/**
 * EdenFS Notifications Client
 * Provides methods to interact with EdenFS notifications via the EdenFS CLI
 */
class EdenFSNotificationsClient extends EventEmitter {
  /** @type {string} */
  mountPoint;
  /** @type {number} */
  timeout;
  /** @type {string} */
  edenBinaryPath;

  DEFAULT_EDENFS_RECONNECT_DELAY_MS = 100;
  MAXIMUM_EDENFS_RECONNECT_DELAY_MS = 60 * 1000;

  constructor(options) {
    super();
    this.mountPoint = options?.mountPoint ?? null;
    this.timeout = options?.timeout ?? 30000; // 30 seconds default timeout
    this.edenBinaryPath = options?.edenBinaryPath ?? 'eden';
  }

  /**
   * Get the current EdenFS status
   * @returns {boolean} Edenfs running/not running
   */
  async getStatus(options = {}) {
    const args = ['status'];

    if (options.useCase) {
      args.push('--use-case', options.useCase);
    } else {
      args.push('--use-case', 'node-client');
    }

    return new Promise((resolve, reject) => {
      execFile(this.edenBinaryPath, args, {timeout: this.timeout}, (error, stdout, stderr) => {
        if (error) {
          reject(
            new Error(
              `Failed to get status: ${error.message}\nStdout: ${stdout}\nStderr: ${stderr}`,
            ),
          );
          return;
        }

        try {
          const result = stdout.trim();
          resolve(result);
        } catch (parseError) {
          reject(new Error(`Failed to parse response: ${parseError.message}\nStdout: ${stdout}`));
        }
      });
    });
  }

  /**
   * Wait until EdenFS is ready
   * @returns {Promise<boolean>} True=Healthy, False=Timeout
   */
  async waitReady(options={}) {
    const maxDelay = this.MAXIMUM_EDENFS_RECONNECT_DELAY_MS;
    let delay = this.DEFAULT_EDENFS_RECONNECT_DELAY_MS;
    const start = Date.now();
    const deadline = start + (options.timeout ?? this.timeout);

    // Helper: sleep for ms
    const sleep = ms => new Promise(res => setTimeout(res, ms));

    // If timeout=0, wait forever
    while (options.timeout == 0 || Date.now() < deadline) {
      try {
        const status = await this.getStatus({useCase: options.useCase ?? undefined});
        // Consider any truthy/non-empty status string as "healthy"
        if (status && typeof status === 'string' && status.trim().length > 0) {
          return true;
        }
      } catch (e) {
        // Swallow and retry with backoff
      }

      // Exponential backoff (capped)
      await sleep(delay);
      delay = Math.min(delay * 2, maxDelay);
    }

    return false;

  }

  /**
   * Get the current EdenFS journal position
   * @param {Object} options - Options for changes-since command
   * @param {string} options.useCase - Use case for the command
   * @param {string} options.mountPoint - Path to the mount point (optional if set in constructor)
   * @returns {Promise<JournalPosition|string>} Journal position
   */
  async getPosition(options = {}) {
    const mountPoint = options?.mountPoint ?? this.mountPoint;
    const args = ['notify', 'get-position'];

    if (options.useCase) {
      args.push('--use-case', options.useCase);
    } else {
      args.push('--use-case', 'node-client');
    }

    if (mountPoint) {
      args.push(mountPoint);
    }

    return new Promise((resolve, reject) => {
      execFile(this.edenBinaryPath, args, {timeout: this.timeout}, (error, stdout, stderr) => {
        if (error) {
          reject(
            new Error(
              `Failed to get position: ${error.message}\nStdout: ${stdout}\nStderr: ${stderr}`,
            ),
          );
          return;
        }

        try {
          const result = stdout.trim();
          resolve(result);
        } catch (parseError) {
          reject(new Error(`Failed to parse response: ${parseError.message}\nStdout: ${stdout}`));
        }
      });
    });
  }

  /**
   * Get changes since a specific journal position
   * @param {Object} options - Options for changes-since command
   * @param {string} options.position - Journal position to start from
   * @param {string} options.useCase - Use case for the command
   * @param {string} options.mountPoint - Path to the mount point (optional if set in constructor)
   * @param {string} options.relativeRoot - Relative root to scope results
   * @param {boolean} options.includeVcsRoots - Include VCS roots in output
   * @param {string[]} options.includedRoots - Included roots in output
   * @param {string[]} options.excludedRoots - Excluded roots in output
   * @param {string[]} options.includedSuffixes - Included suffixes in output
   * @param {string[]} options.excludedSuffixes - Excluded suffixes in output
   * @param {boolean} options.json - Return JSON format (default: true)
   * @param {string[]} options.deferredStates - States to wait for deassertion
   * @returns {Promise<Object|string>} Changes since position
   */
  async getChangesSince(options = {}) {
    const mountPoint = options?.mountPoint ?? this.mountPoint;
    const args = ['notify', 'changes-since'];

    if (options.useCase) {
      args.push('--use-case', options.useCase);
    } else {
      args.push('--use-case', 'node-client');
    }

    if (options.position) {
      args.push(
        '--position',
        typeof options.position === 'string' ? options.position : JSON.stringify(options.position),
      );
    }

    if (options.relativeRoot) {
      args.push('--relative-root', options.relativeRoot);
    }

    if (options.includeVcsRoots) {
      args.push('--include-vcs-roots');
    }

    if (options.includedRoots) {
      options.includedRoots.forEach(root => {
        args.push('--included-roots', root);
      });
    }

    if (options.excludedRoots) {
      options.excludedRoots.forEach(root => {
        args.push('--excluded-roots', root);
      });
    }

    if (options.includedSuffixes) {
      options.includedSuffixes.forEach(suffix => {
        args.push('--included-suffixes', suffix);
      });
    }

    if (options.excludedSuffixes) {
      options.excludedSuffixes.forEach(suffix => {
        args.push('--excluded-suffixes', suffix);
      });
    }

    if (options.deferredStates) {
      options.deferredStates.forEach(state => {
        args.push('--deferred-states', state);
      });
    }

    args.push('--json');
    args.push('--formatted-position');

    if (mountPoint) {
      args.push(mountPoint);
    }

    return new Promise((resolve, reject) => {
      execFile(this.edenBinaryPath, args, {timeout: this.timeout}, (error, stdout, stderr) => {
        if (error) {
          reject(new Error(`Failed to get changes: ${error.message}\nStderr: ${stderr}`));
          return;
        }

        try {
          const result = JSON.parse(stdout.trim());
          resolve(result);
        } catch (parseError) {
          reject(new Error(`Failed to parse response: ${parseError.message}`));
        }
      });
    });
  }

  /**
   * Subscribe to filesystem changes
   * @param {Object} options - Options for subscription
   * @param {string} options.position - Journal position to start from (optional)
   * @param {string} options.useCase - Use case for the command
   * @param {string} options.mountPoint - Path to the mount point (optional if set in constructor)
   * @param {number} options.throttle - Throttle in milliseconds between events (default: 0)
   * @param {string} options.relativeRoot - Relative root to scope results
   * @param {boolean} options.includeVcsRoots - Include VCS roots in output
   * @param {string[]} options.includedRoots - Included roots in output
   * @param {string[]} options.excludedRoots - Excluded roots in output
   * @param {string[]} options.includedSuffixes - Included suffixes in output
   * @param {string[]} options.excludedSuffixes - Excluded suffixes in output
   * @param {string[]} options.deferredStates - States to wait for deassertion
   * @param {CommandCallback} callback
   * @returns {EdenFSSubscription} Subscription object
   */
  subscribe(options = {}, callback = () => {}) {
    options['edenBinaryPath'] = this.edenBinaryPath;
    let sub = new EdenFSSubscription(this, options, callback);
    sub.on('change', change => {
      callback(null, change);
    });
    sub.on('error', error => {
      callback(error, null);
    });
    sub.on('close', () => {
      // Received when the underlying gets killed, pass double null to indicate
      // this since no error or message is available
      callback(null, null);
    });
    return sub;
  }

  /**
   * Enter a specific state
   * @param {string} state - State name to enter
   * @param {Object} options - Options for enterState command
   * @param {number} [options.duration] - Duration in seconds to maintain state
   * @param {string} [options.useCase] - Use case for the command
   * @param {string} options.mountPoint - Path to the mount point (optional if set in constructor)
   * @returns {Promise<void>}
   */
  async enterState(state, options = {}) {
    const mountPoint = options?.mountPoint ?? this.mountPoint;
    if (!state || typeof state !== 'string') {
      throw new Error('State name must be a non-empty string');
    }

    const args = ['notify', 'enter-state', state];
    if (options.duration !== undefined) {
      args.push('--duration', options.duration.toString());
    }

    if (options.useCase) {
      args.push('--use-case', options.useCase);
    } else {
      args.push('--use-case', 'node-client');
    }

    if (mountPoint) {
      args.push(mountPoint);
    }

    return new Promise((resolve, reject) => {
      execFile(this.edenBinaryPath, args, {timeout: this.timeout}, (error, stdout, stderr) => {
        if (error) {
          reject(
            new Error(
              `Failed to enter state: ${error.message}\nStdout: ${stdout}\nStderr: ${stderr}`,
            ),
          );
          return;
        }
        resolve();
      });
    });
  }
}

/**
 * EdenFS Subscription
 * Handles real-time filesystem change notifications
 */
class EdenFSSubscription extends EventEmitter {
  /** @type {EdenFSNotificationsClient} */
  client;
  /** @type {Object} */
  options;
  /** @type {any} */
  process;
  /** @type {string} */
  edenBinaryPath;
  /** @type {string} */
  errData;
  /** @type {NodeJS.Timeout | null} */
  killTimeout;

  constructor(client, options = {}) {
    super();
    this.client = client;
    this.options = options;
    this.process = null;
    this.edenBinaryPath = options?.edenBinaryPath ?? 'eden';
    this.errData = '';
    this.killTimeout = null;
  }

  /**
   * Start the subscription
   * @returns {Promise<void>}
   */
  async start() {
    const mountPoint = this.options.mountPoint || this.client.mountPoint;
    const args = ['notify', 'changes-since', '--subscribe', '--json', '--formatted-position'];

    if (this.options.useCase) {
      args.push('--use-case', this.options.useCase);
    } else {
      args.push('--use-case', 'node-client');
    }

    if (this.options.position) {
      args.push(
        '--position',
        typeof this.options.position === 'string'
          ? this.options.position
          : JSON.stringify(this.options.position),
      );
    }

    if (this.options.throttle !== undefined) {
      args.push('--throttle', this.options.throttle.toString());
    }

    if (this.options.relativeRoot) {
      args.push('--relative-root', this.options.relativeRoot);
    }

    if (this.options.includeVcsRoots) {
      args.push('--include-vcs-roots');
    }

    if (this.options.includedRoots) {
      this.options.includedRoots.forEach(root => {
        args.push('--included-roots', root);
      });
    }

    if (this.options.excludedRoots) {
      this.options.excludedRoots.forEach(root => {
        args.push('--excluded-roots', root);
      });
    }

    if (this.options.includedSuffixes) {
      this.options.includedSuffixes.forEach(suffix => {
        args.push('--included-suffixes', suffix);
      });
    }

    if (this.options.excludedSuffixes) {
      this.options.excludedSuffixes.forEach(suffix => {
        args.push('--excluded-suffixes', suffix);
      });
    }

    if (this.options.deferredStates) {
      this.options.deferredStates.forEach(state => {
        args.push('--deferred-states', state);
      });
    }

    if (mountPoint) {
      args.push(mountPoint);
    }

    return new Promise((resolve, reject) => {
      this.process = spawn(this.edenBinaryPath, args, {
        stdio: ['pipe', 'pipe', 'pipe'],
      });

      let buffer = '';

      const readline = require('readline');
      const rl = readline.createInterface({input: this.process.stdout});

      rl.on('line', line => {
        if (line.trim()) {
          try {
            const event = JSON.parse(line);
            this.emit('change', event);
          } catch (error) {
            this.emit('error', new Error(`Failed to parse event ${line}: ${error.message}`));
          }
        }
      });

      this.process.stderr.on('data', data => {
        this.errData += data.toString() + '\n';
      });

      this.process.on('close', (code, signal) => {
        if (code !== 0 && code !== null) {
          this.emit(
            'error',
            new Error(`EdenFS process exited with code ${code}\nstderr: ${this.errData}`),
          );
        } else if (signal !== null && signal !== 'SIGTERM') {
          this.emit('error', new Error(`EdenFS process killed with signal ${signal}`));
        } else {
          this.emit('close');
        }
      });

      this.process.on('error', error => {
        this.emit('error', error);
        reject(error);
      });

      this.process.on('spawn', () => {
        resolve();
      });

      this.process.on('exit', (code, signal) => {
        if (this.killTimeout !== null) {
          clearTimeout(this.killTimeout);
          this.killTimeout = null;
        }
        this.emit('exit');
      });
    });
  }

  /**
   * Stop the subscription
   */
  stop() {
    if (this.process) {
      this.process.kill('SIGTERM');
      this.killTimeout = setTimeout(() => {
        this.process.kill('SIGKILL');
      }, 500);
    }
  }
}

/**
 * Utility functions for working with EdenFS notify data
 */
class EdenFSUtils {
  /**
   * Convert byte array path to string
   * @param {number[]} pathBytes - Array of byte values representing a path
   * @returns {string} Path string
   */
  static bytesToPath(pathBytes) {
    return Buffer.from(pathBytes).toString('utf8');
  }

  /**
   * Convert byte array to hex string
   * @param {number[]} bytes - Array of byte values
   * @returns {string} Hexadecimal string
   */
  static bytesToHex(bytes) {
    return Buffer.from(bytes).toString('hex');
  }

  /**
   * Extract file type from change
   * @param {Object} change - change object
   * @returns {{string}} File Type
   */
  static extractFileType(smallChange) {
    if (smallChange.Added && smallChange.Added.file_type) {
      return smallChange.Added.file_type;
    } else if (smallChange.Modified && smallChange.Modified.file_type) {
      return smallChange.Modified.file_type;
    } else if (smallChange.Removed && smallChange.Removed.file_type) {
      return smallChange.Removed.file_type;
    } else if (smallChange.Renamed) {
      return smallChange.Renamed.file_type;
    } else if (smallChange.Replaced) {
      return smallChange.Replaced.file_type;
    }
  }

  /**
   * Extract file path(s) from change
   * @param {Object} change - change object
   * @returns {{string, string | undefined}} First file path, and possible second file path
   */
  static extractPath(smallChange) {
    if (smallChange.Added && smallChange.Added.path) {
      return [this.bytesToPath(smallChange.Added.path), undefined];
    } else if (smallChange.Modified && smallChange.Modified.path) {
      return [this.bytesToPath(smallChange.Modified.path), undefined];
    } else if (smallChange.Removed && smallChange.Removed.path) {
      return [this.bytesToPath(smallChange.Removed.path), undefined];
    } else if (smallChange.Renamed) {
      return [this.bytesToPath(smallChange.Renamed.from), this.bytesToPath(smallChange.Renamed.to)];
    } else if (smallChange.Replaced) {
      return [
        this.bytesToPath(smallChange.Replaced.from),
        this.bytesToPath(smallChange.Replaced.to),
      ];
    } else {
      return ['', undefined];
    }
  }

  /**
   * Extract file paths from changes
   * @param {Object[]} changes - Array of change objects
   * @returns {string[]} Array of file paths
   */
  static extractPaths(changes) {
    const paths = [];

    changes.forEach(change => {
      if (change.SmallChange) {
        let [path1, path2] = this.extractPath(change.SmallChange);
        if (path1) {
          paths.push(path1);
        }
        if (path2) {
          paths.push(path2);
        }
      }
    });

    return paths;
  }

  /**
   * Get change type from a change object
   * @param {Object} change - Change object
   * @returns {string} Change type
   */
  static getChangeType(change) {
    if (change.SmallChange) {
      const smallChange = change.SmallChange;

      if (smallChange.Added) return 'added';
      if (smallChange.Modified) return 'modified';
      if (smallChange.Removed) return 'removed';
      if (smallChange.Renamed) return 'renamed';
      if (smallChange.Replaced) return 'replaced';
    } else if (change.LargeChange) {
      const largeChange = change.LargeChange;
      if (largeChange.DirectoryRenamed) return 'directory renamed';
      if (largeChange.CommitTransition) return 'commit transition';
      if (largeChange.LostChange) return 'lost change';
    } else if (change.StateChange) {
      const stateChange = change.StateChange;
      if (stateChange.StateEntered) return 'state entered';
      if (stateChange.StateLeft) return 'state left';
    }

    return 'unknown';
  }
}

module.exports = {
  EdenFSNotificationsClient,
  EdenFSSubscription,
  EdenFSUtils,
};
