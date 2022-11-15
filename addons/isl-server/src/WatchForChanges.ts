/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PageFocusTracker} from './PageFocusTracker';
import type {Logger} from './logger';
import type {PageVisibility, RepoInfo} from 'isl/src/types';

import {ONE_MINUTE_MS} from './constants';
import {Watchman} from './watchman';
import path from 'path';
import {debounce} from 'shared/debounce';

const DEFAULT_POLL_INTERVAL = 5 * ONE_MINUTE_MS;
const VISIBLE_POLL_INTERVAL = 1 * ONE_MINUTE_MS;
const FOCUSED_POLL_INTERVAL = 0.25 * ONE_MINUTE_MS;
const ON_FOCUS_REFETCH_THROTTLE = 10_000;
const ON_VISIBLE_REFETCH_THROTTLE = 20_000;

export type KindOfChange = 'uncommitted changes' | 'commits' | 'merge conflicts' | 'everything';

/**
 * Handles watching for changes to files on disk which should trigger refetching data,
 * and polling for changes when watching is not reliable.
 */
export class WatchForChanges {
  static WATCHMAN_DEFER = `hg.update`; // TODO: update to sl
  public watchman: Watchman;

  private disposables: Array<() => unknown> = [];

  constructor(
    private repoInfo: RepoInfo,
    private logger: Logger,
    private pageFocusTracker: PageFocusTracker,
    private changeCallback: (kind: KindOfChange) => unknown,
    watchman?: Watchman | undefined,
  ) {
    this.watchman = watchman ?? new Watchman(logger);

    this.setupWatchmanSubscriptions();
    this.setupPolling();
    this.pageFocusTracker.onChange(this.poll.bind(this));
  }

  private timeout: NodeJS.Timeout | undefined;
  private lastFetch = new Date().valueOf();

  /**
   * Combine different signals to determine what interval to poll for information
   */
  private setupPolling() {
    this.timeout = setTimeout(this.poll, DEFAULT_POLL_INTERVAL);
  }

  /**
   * Re-trigger fetching data from the repository,
   * depending on how recently that data was last fetched,
   * and whether any ISL windows are focused or visible.
   *
   * This function calls itself on an interval to check whether we should fetch changes,
   * but it can also be called in response to events like focus being gained.
   */
  public poll = (kind?: PageVisibility | 'force') => {
    // calculate how long we'd like to be waiting from what we know of the windows.
    let desiredNextTickTime = DEFAULT_POLL_INTERVAL;
    if (this.watchman.status !== 'healthy') {
      if (this.pageFocusTracker.hasPageWithFocus()) {
        desiredNextTickTime = FOCUSED_POLL_INTERVAL;
      } else if (this.pageFocusTracker.hasVisiblePage()) {
        desiredNextTickTime = VISIBLE_POLL_INTERVAL;
      }
    }

    const now = Date.now();
    const elapsedTickTime = now - this.lastFetch;

    if (
      kind === 'force' ||
      // we've been waiting longer than desired
      elapsedTickTime >= desiredNextTickTime ||
      // the moment a window gains focus or visibility, consider polling immediately
      (kind === 'focused' && elapsedTickTime >= ON_FOCUS_REFETCH_THROTTLE) ||
      (kind === 'visible' && elapsedTickTime >= ON_VISIBLE_REFETCH_THROTTLE)
    ) {
      // it's time to fetch
      this.changeCallback('everything');
      this.lastFetch = Date.now();

      clearTimeout(this.timeout);
      this.timeout = setTimeout(this.poll, desiredNextTickTime);
    } else {
      // we have some time left before we we would expect to need to poll, schedule next poll
      clearTimeout(this.timeout);
      this.timeout = setTimeout(this.poll, desiredNextTickTime - elapsedTickTime);
    }
  };

  private async setupWatchmanSubscriptions() {
    const {repoRoot, dotdir} = this.repoInfo;

    if (repoRoot == null || dotdir == null) {
      this.logger.error(`skipping watchman subscription since ${repoRoot} is not a repository`);
      return;
    }
    const relativeDotdir = path.relative(repoRoot, dotdir);

    const FILE_CHANGE_WATCHMAN_SUBSCRIPTION = 'sapling-smartlog-file-change';
    const DIRSTATE_WATCHMAN_SUBSCRIPTION = 'sapling-smartlog-dirstate-change';
    try {
      const handleRepositoryStateChange = debounce(() => {
        // if the repo changes, also recheck files. E.g. if you commit, your uncommitted changes will also change.
        this.changeCallback('everything');

        // reset timer for polling
        this.lastFetch = new Date().valueOf();
      }, 100); // debounce so that multiple quick changes don't trigger multiple fetches for no reason
      const dirstateSubscription = await this.watchman.watchDirectoryRecursive(
        repoRoot,
        DIRSTATE_WATCHMAN_SUBSCRIPTION,
        {
          fields: ['name'],
          expression: [
            'name',
            [
              `${relativeDotdir}/bookmarks.current`,
              `${relativeDotdir}/bookmarks`,
              `${relativeDotdir}/dirstate`,
              `${relativeDotdir}/merge`,
            ],
            'wholename',
          ],
          defer: [WatchForChanges.WATCHMAN_DEFER],
          empty_on_fresh_instance: true,
        },
      );
      dirstateSubscription.emitter.on('change', changes => {
        if (changes.includes(`${relativeDotdir}/merge`)) {
          this.changeCallback('merge conflicts');
        }
        if (changes.includes(`${relativeDotdir}/dirstate`)) {
          handleRepositoryStateChange();
        }
      });
      dirstateSubscription.emitter.on('fresh-instance', handleRepositoryStateChange);

      const handleUncommittedChanges = () => {
        this.changeCallback('uncommitted changes');

        // reset timer for polling
        this.lastFetch = new Date().valueOf();
      };
      const uncommittedChangesSubscription = await this.watchman.watchDirectoryRecursive(
        repoRoot,
        FILE_CHANGE_WATCHMAN_SUBSCRIPTION,
        {
          // We only need to know that a change happened (not the list of files) so that we can trigger `status`
          fields: ['name'],
          expression: [
            'allof',
            // This watchman subscription is used to determine when and which
            // files to fetch new statuses for. There is no reason to include
            // directories in these updates, and in fact they may make us overfetch
            // statuses.
            // This line restricts this subscription to only return files.
            ['type', 'f'],
            ['not', ['dirname', relativeDotdir]],
            // Even though we tell it not to match .sl, modifying a file inside .sl
            // will emit an event for the folder itself, which we want to ignore.
            ['not', ['match', relativeDotdir, 'basename']],
          ],
          defer: [WatchForChanges.WATCHMAN_DEFER],
          empty_on_fresh_instance: true,
        },
      );
      uncommittedChangesSubscription.emitter.on('change', handleUncommittedChanges);
      uncommittedChangesSubscription.emitter.on('fresh-instance', handleUncommittedChanges);

      this.disposables.push(() => {
        this.logger.log('unsubscribe watchman');
        this.watchman.unwatch(repoRoot, DIRSTATE_WATCHMAN_SUBSCRIPTION);
        this.watchman.unwatch(repoRoot, FILE_CHANGE_WATCHMAN_SUBSCRIPTION);
      });
    } catch (err) {
      this.logger.error('failed to setup watchman subscriptions', err);
    }
  }

  public dispose() {
    this.disposables.forEach(dispose => dispose());
    if (this.timeout) {
      clearTimeout(this.timeout);
      this.timeout = undefined;
    }
  }
}
