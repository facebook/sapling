/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PageFocusTracker} from './PageFocusTracker';
import type {Logger} from './logger';
import type {PageVisibility, RepoInfo} from 'isl/src/types';

import {stagedThrottler} from './StagedThrottler';
import {ONE_MINUTE_MS} from './constants';
import {Watchman} from './watchman';
import path from 'path';
import {debounce} from 'shared/debounce';

const DEFAULT_POLL_INTERVAL = 5 * ONE_MINUTE_MS;
// When the page is hidden, aggressively reduce polling.
const HIDDEN_POLL_INTERVAL = 60 * ONE_MINUTE_MS;
// When visible or focused, poll frequently
const VISIBLE_POLL_INTERVAL = 1 * ONE_MINUTE_MS;
const FOCUSED_POLL_INTERVAL = 0.25 * ONE_MINUTE_MS;
const ON_FOCUS_REFETCH_THROTTLE = 10_000;
const ON_VISIBLE_REFETCH_THROTTLE = 20_000;

export type KindOfChange = 'uncommitted changes' | 'commits' | 'merge conflicts' | 'everything';
export type PollKind = PageVisibility | 'force';

/**
 * Handles watching for changes to files on disk which should trigger refetching data,
 * and polling for changes when watching is not reliable.
 */
export class WatchForChanges {
  static WATCHMAN_DEFER = `hg.update`; // TODO: update to sl
  public watchman: Watchman;

  private dirstateDisposables: Array<() => unknown> = [];
  private watchmanDisposables: Array<() => unknown> = [];

  constructor(
    private repoInfo: RepoInfo,
    private logger: Logger,
    private pageFocusTracker: PageFocusTracker,
    private changeCallback: (kind: KindOfChange, pollKind?: PollKind) => unknown,
    watchman?: Watchman | undefined,
  ) {
    this.watchman = watchman ?? new Watchman(logger);

    // Watch dirstate right away for commit changes
    this.setupDirstateSubscriptions();
    this.setupPolling();
    this.pageFocusTracker.onChange(this.poll.bind(this));
    // poll right away so we get data immediately, without waiting for timeout on startup
    this.poll('force');
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
  public poll = (kind?: PollKind) => {
    // calculate how long we'd like to be waiting from what we know of the windows.
    let desiredNextTickTime = DEFAULT_POLL_INTERVAL;
    if (this.watchman.status !== 'healthy') {
      if (this.pageFocusTracker.hasPageWithFocus()) {
        desiredNextTickTime = FOCUSED_POLL_INTERVAL;
      } else if (this.pageFocusTracker.hasVisiblePage()) {
        desiredNextTickTime = VISIBLE_POLL_INTERVAL;
      }
    } else {
      // if watchman is working normally, and we're not visible, don't poll nearly as often
      if (!this.pageFocusTracker.hasPageWithFocus() && !this.pageFocusTracker.hasVisiblePage()) {
        desiredNextTickTime = HIDDEN_POLL_INTERVAL;
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
      this.changeCallback('everything', kind);
      this.lastFetch = Date.now();

      clearTimeout(this.timeout);
      this.timeout = setTimeout(this.poll, desiredNextTickTime);
    } else {
      // we have some time left before we we would expect to need to poll, schedule next poll
      clearTimeout(this.timeout);
      this.timeout = setTimeout(this.poll, desiredNextTickTime - elapsedTickTime);
    }
  };

  private async setupDirstateSubscriptions() {
    if (this.repoInfo.type !== 'success') {
      return;
    }
    const {repoRoot, dotdir} = this.repoInfo;

    if (repoRoot == null || dotdir == null) {
      this.logger.error(`skipping dirstate subscription since ${repoRoot} is not a repository`);
      return;
    }
    const relativeDotdir = path.relative(repoRoot, dotdir);

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

      this.dirstateDisposables.push(() => {
        this.logger.log('unsubscribe dirstate watcher');
        this.watchman.unwatch(repoRoot, DIRSTATE_WATCHMAN_SUBSCRIPTION);
      });
    } catch (err) {
      this.logger.error('failed to setup dirstate subscriptions', err);
    }
  }

  /**
   * Some Watchmans subscriptions should only activate when ISL is actually opened.
   * On platforms like vscode, it's possible to create a Repository without actually opening ISL.
   * In those cases, we only want the minimum set of subscriptions to be active.
   * We care about the dirstate watcher, but not the watchman subscriptions in that case.
   */
  public async setupWatchmanSubscriptions() {
    if (this.repoInfo.type !== 'success') {
      return;
    }
    const {repoRoot, dotdir} = this.repoInfo;

    if (repoRoot == null || dotdir == null) {
      this.logger.error(`skipping watchman subscription since ${repoRoot} is not a repository`);
      return;
    }
    const relativeDotdir = path.relative(repoRoot, dotdir);
    // if working from a git clone, the dotdir lives in .git/sl,
    // but we need to ignore changes in .git in our watchman subscriptions
    const outerDotDir =
      relativeDotdir.indexOf(path.sep) >= 0 ? path.dirname(relativeDotdir) : relativeDotdir;

    const FILE_CHANGE_WATCHMAN_SUBSCRIPTION = 'sapling-smartlog-file-change';
    try {
      // In some bad cases, a file may not be getting ignored by watchman properly,
      // and ends up constantly triggering the watchman subscription.
      // Incrementally increase the throttling of events to avoid spamming `status`.
      // This does mean "legit" changes will start being missed.
      // TODO: can we scan the list of changes and build a list of files that are overfiring, then send those to the UI as a warning?
      // This would allow a user to know it's happening and possibly fix it for their repo by adding it to a .watchmanconfig.
      const handleUncommittedChanges = stagedThrottler(
        [
          {
            throttleMs: 0,
            numToNextStage: 5,
            resetAfterMs: 5_000,
            onEnter: () => {
              this.logger.info('no longer throttling uncommitted changes');
            },
          },
          {
            throttleMs: 5_000,
            numToNextStage: 10,
            resetAfterMs: 20_000,
            onEnter: () => {
              this.logger.info('slightly throttling uncommitted changes');
            },
          },
          {
            throttleMs: 30_000,
            resetAfterMs: 30_000,
            onEnter: () => {
              this.logger.info('aggressively throttling uncommitted changes');
            },
          },
        ],
        () => {
          this.changeCallback('uncommitted changes');

          // reset timer for polling
          this.lastFetch = new Date().valueOf();
        },
      );
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
            ['not', ['dirname', outerDotDir]],
            // Even though we tell it not to match .sl, modifying a file inside .sl
            // will emit an event for the folder itself, which we want to ignore.
            ['not', ['match', outerDotDir, 'basename']],
          ],
          defer: [WatchForChanges.WATCHMAN_DEFER],
          empty_on_fresh_instance: true,
        },
      );
      uncommittedChangesSubscription.emitter.on('change', handleUncommittedChanges);
      uncommittedChangesSubscription.emitter.on('fresh-instance', handleUncommittedChanges);

      this.watchmanDisposables.push(() => {
        this.logger.log('unsubscribe watchman');
        this.watchman.unwatch(repoRoot, FILE_CHANGE_WATCHMAN_SUBSCRIPTION);
      });
    } catch (err) {
      this.logger.error('failed to setup watchman subscriptions', err);
    }
  }

  public disposeWatchmanSubscriptions() {
    this.watchmanDisposables.forEach(dispose => dispose());
  }

  public dispose() {
    this.dirstateDisposables.forEach(dispose => dispose());
    this.disposeWatchmanSubscriptions();
    if (this.timeout) {
      clearTimeout(this.timeout);
      this.timeout = undefined;
    }
  }
}
