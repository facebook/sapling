/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Watchman} from '../watchman';
import type {Client} from 'fb-watchman';
import type {RepoInfo} from 'isl/src/types';

import {PageFocusTracker} from '../PageFocusTracker';
import {WatchForChanges} from '../WatchForChanges';
import {ONE_MINUTE_MS} from '../constants';
import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {mockLogger} from 'shared/testUtils';

jest.mock('fb-watchman', () => {
  // make a fake watchman object which returns () => undefined for every property
  // so we don't need to manually mock every function watchman provides.
  class FakeWatchman {
    constructor() {
      return new Proxy(this, {
        get: () => () => undefined,
      });
    }
  }
  return {
    Client: FakeWatchman,
  };
});

describe('WatchForChanges', () => {
  const mockInfo: RepoInfo = {
    command: 'sl',
    repoRoot: '/testRepo',
    dotdir: '/testRepo/.sl',
    codeReviewSystem: {type: 'unknown'},
    pullRequestDomain: undefined,
  };

  let focusTracker: PageFocusTracker;
  const onChange = jest.fn();
  let watch: WatchForChanges;

  beforeEach(() => {
    jest.useFakeTimers();
    onChange.mockClear();
    focusTracker = new PageFocusTracker();
    watch = new WatchForChanges(mockInfo, mockLogger, focusTracker, onChange);
    // pretend watchman is not running for most tests
    (watch.watchman.status as string) = 'errored';
  });

  afterEach(() => {
    watch.dispose();
  });

  afterAll(() => {
    jest.useRealTimers();
  });

  it('polls for changes on an interval', () => {
    // |-----------1-----------2-----------3-----------4-----------5-----------6 (minutes)
    //                                                                  ^ poll
    expect(onChange).not.toHaveBeenCalled();
    jest.advanceTimersByTime(5.5 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith('everything');
  });

  it('polls more often when the page is visible', () => {
    // |-----------1-----------2---- (minutes)
    //       |     ^     |     ^     (poll)
    //       0    poll   1           (times fetched)
    focusTracker.setState('page0', 'visible');
    onChange.mockClear(); // ignore immediate visibility change poll
    expect(onChange).not.toHaveBeenCalled();
    jest.advanceTimersByTime(0.5 * ONE_MINUTE_MS);
    expect(onChange).not.toHaveBeenCalled();
    jest.advanceTimersByTime(0.75 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(1);
  });
  it('polls more often when the page is focused', () => {
    // |-----------1-----------2---- (minutes)
    //  | ^| ^  ^  ^  ^  ^  ^  ^  ^  (poll)
    //  0  1                         (times fetched)
    focusTracker.setState('page0', 'focused');
    onChange.mockClear(); // ignore immediate focus change poll
    expect(onChange).not.toHaveBeenCalled();
    jest.advanceTimersByTime(0.15 * ONE_MINUTE_MS);
    expect(onChange).not.toHaveBeenCalled();
    jest.advanceTimersByTime(0.1 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it('polls the moment visibility is gained', () => {
    // |-----------1-----*-----2-----------3----------- (minutes)
    //             |     ||       |  ^   |        ^     (poll)
    //             0     |1       1      2              (times fetched)
    //                visible
    //        (resets interval at 1min)
    jest.advanceTimersByTime(1.5 * ONE_MINUTE_MS);
    expect(onChange).not.toHaveBeenCalled();
    focusTracker.setState('page0', 'visible');
    expect(onChange).toHaveBeenCalledTimes(1);
    jest.advanceTimersByTime(0.45 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(1);
    jest.advanceTimersByTime(0.6 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it('debounces additional polling when focus is gained', () => {
    // |-----------1-----*--*--2-*---------3----------- (minutes)
    //             |     || |    ||  ^   |        ^     (poll)
    //             0     |1 |    |1      2              (times fetched)
    //            visible^  |    ^visible (debounce)
    //                     hide
    jest.advanceTimersByTime(1.5 * ONE_MINUTE_MS);
    expect(onChange).not.toHaveBeenCalled();
    focusTracker.setState('page0', 'visible');
    expect(onChange).toHaveBeenCalledTimes(1);
    jest.advanceTimersByTime(0.1 * ONE_MINUTE_MS);
    focusTracker.setState('page0', 'hidden');
    jest.advanceTimersByTime(0.05 * ONE_MINUTE_MS);
    focusTracker.setState('page0', 'visible'); // debounced to not fetch again
    expect(onChange).toHaveBeenCalledTimes(1);
    jest.advanceTimersByTime(0.3 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(1);
    jest.advanceTimersByTime(0.6 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it('polls at higher frequency if any page is focused', () => {
    // |-----------1-----*-*---2-----------3-- (minutes)
    //             |     ||| ^| ^| ^  ^  ^     (poll)
    //             0     |1|  2  3             (times fetched)
    //            hidden | |hidden
    //            focused^ |hidden
    //            hidden   ^focused
    jest.advanceTimersByTime(1.5 * ONE_MINUTE_MS);
    expect(onChange).not.toHaveBeenCalled();
    focusTracker.setState('page0', 'hidden');
    focusTracker.setState('page1', 'hidden');
    focusTracker.setState('page2', 'hidden');
    focusTracker.setState('page1', 'focused');
    expect(onChange).toHaveBeenCalledTimes(1);
    jest.advanceTimersByTime(0.3 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(2);
    focusTracker.setState('page0', 'focused'); // since 1 is still focused, this does not immediately poll
    focusTracker.setState('page1', 'hidden');
    expect(onChange).toHaveBeenCalledTimes(2);
    jest.advanceTimersByTime(0.3 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(3);
  });

  it('clears out previous intervals', () => {
    // |-----------1-----*-----2-----*-----3---   ...   --7-----------8-- (minutes)
    //             |     |  ^  ^  ^  ^                          ^         (poll)
    //             0     1           5                          6         (times fetched)
    //            focused^           ^hidden
    jest.advanceTimersByTime(1.5 * ONE_MINUTE_MS);
    expect(onChange).not.toHaveBeenCalled();
    focusTracker.setState('page0', 'focused');
    expect(onChange).toHaveBeenCalledTimes(1);
    jest.advanceTimersByTime(1 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(5);
    focusTracker.setState('page0', 'hidden');
    // fast focused interval is removed, and we revert to 5 min interval
    jest.advanceTimersByTime(5 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(6);
  });

  it('polls less when watchman appears healthy', () => {
    // |*----------1-----------2-----------3-----------4-----------5-----------6 (minutes)
    //  |                                                               ^ poll
    //  focused

    (watch.watchman.status as string) = 'healthy';
    focusTracker.setState('page0', 'focused');
    expect(onChange).toHaveBeenCalledTimes(0);
    jest.advanceTimersByTime(5.5 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it('results from watchman reset polling timers', async () => {
    // |-----------1-----------2----- ... ----6-----------7 (minutes)
    //                  |                           ^       (poll)
    //            watchman result

    watch.dispose(); // don't use pre-existing WatchForChanges
    const emitter1 = new TypedEventEmitter();
    const emitter2 = new TypedEventEmitter();
    const mockWatchman: Watchman = {
      client: {} as unknown as Client,
      status: 'initializing',
      watchDirectoryRecursive: jest
        .fn()
        .mockImplementationOnce(() => {
          return Promise.resolve({emitter: emitter1});
        })
        .mockImplementationOnce(() => {
          return Promise.resolve({emitter: emitter2});
        }),
      unwatch: jest.fn(),
    } as unknown as Watchman;
    watch = new WatchForChanges(mockInfo, mockLogger, focusTracker, onChange, mockWatchman);

    // wait an actual async tick so mock subscriptions are set up
    const setImmediate = jest.requireActual('timers').setImmediate;
    await new Promise(res => setImmediate(() => res(undefined)));

    jest.advanceTimersByTime(1.5 * ONE_MINUTE_MS);
    expect(onChange).toHaveBeenCalledTimes(0);
    emitter2.emit('change', undefined);
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith('uncommitted changes');
    jest.advanceTimersByTime(4.0 * ONE_MINUTE_MS); // original timer didn't cause a poll
    expect(onChange).toHaveBeenCalledTimes(1);
    jest.advanceTimersByTime(2.0 * ONE_MINUTE_MS); // 5 minutes after watchman change, a new poll occurred
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenCalledWith('everything');
  });
});
