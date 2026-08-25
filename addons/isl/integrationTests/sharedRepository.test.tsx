/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from 'isl-server/src/Repository';

import fs from 'node:fs';

import {act, waitFor} from '@testing-library/react';
import {onClientConnection} from 'isl-server/src/index';
import {StdoutLogger} from 'isl-server/src/logger';
import {repositoryCache} from 'isl-server/src/RepositoryCache';
import {serializeToString} from '../src/serialize';
import {initRepo} from './setup';

/**
 * Several ISL windows on one clone is the normal case for anything that opens ISL
 * programmatically, not an edge case. Per-repo server-side caches (diff summaries, CI signal
 * counts) are only worth having if a second window actually reaches the first window's cache,
 * and that depends on both connections resolving to the same `Repository` instance.
 *
 * The unit tests cover those caches thoroughly, but every one of them holds a single cache
 * object directly, so none can tell you whether two *connections* ever share one.
 *
 * Note this cannot be checked by reloading one window: the first connection's reference is
 * released before the second is taken, so the repo may legitimately be torn down in between.
 * Only overlapping connections exercise sharing.
 *
 * Scope: this covers the synchronous reuse path (`cwd === repoRoot`). The post-`getRepoInfo`
 * recheck that serves windows opened from a subdirectory has its own unit coverage in
 * `isl-server/src/__tests__/RepositoryCache.test.ts`.
 */
describe('multiple simultaneous ISL clients', () => {
  /**
   * Opens a second connection to `repoDir` alongside whatever `initRepo` already established.
   *
   * Resolves only once the server has posted `repoInfo` to this new client.
   *
   * Waiting is load-bearing, but NOT because a silent connection fails to resolve a repo — it
   * does: `onClientConnection` calls `setActiveRepoForCwd` unconditionally, and on the reuse
   * fast path `found.ref()` runs synchronously, so the refcount already holds with zero messages
   * sent. The reason to wait is the *other* path: when reuse is broken, resolving a second
   * Repository is asynchronous, so without this barrier the assertions would run before that
   * second repo exists and would pass against the first client's — which is exactly how an
   * earlier version of this test survived a reuse-disabled mutant.
   */
  async function openSecondClient(repoDir: string, command: string) {
    const received: Array<string> = [];
    let toServer: ((event: Buffer, isBinary: boolean) => void | Promise<void>) | undefined;

    const dispose = onClientConnection({
      cwd: repoDir,
      version: 'integration-test-second-client',
      command,
      logger: new StdoutLogger(),
      appMode: {mode: 'isl'},
      postMessage(message: string): Promise<boolean> {
        received.push(message);
        return Promise.resolve(true);
      },
      onDidReceiveMessage(handler) {
        toServer = handler;
        return {dispose: () => (toServer = undefined)};
      },
    });

    const send = (message: unknown) =>
      toServer?.(Buffer.from(serializeToString(message as never), 'utf8'), false);

    // A connection that never speaks never resolves a repo. The real client sends these on
    // mount, and `requestRepoInfo` is what makes the server answer with `repoInfo` — which is
    // our only signal that this connection finished resolving a Repository of its own.
    send({type: 'clientReady'});
    send({type: 'requestRepoInfo'});

    await waitFor(
      () => {
        expect(received.some(m => m.includes('"type":"repoInfo"'))).toBe(true);
      },
      {timeout: 30_000},
    );

    return {dispose, received, send};
  }

  function cachedRepo(repoDir: string): Repository {
    const repo = repositoryCache.cachedRepositoryForPath(repoDir);
    expect(repo).toBeDefined();
    return repo as Repository;
  }

  it('serves a second connection from the same Repository as the first', async () => {
    const {repoDir: rawRepoDir, cleanup} = await initRepo();
    // `sl root` reports the realpath, and `cachedRepositoryForPath` prefix-matches literally with
    // no normalisation — so on macOS, where os.tmpdir() is /var/folders/... symlinked to
    // /private/var/folders/..., looking up the un-resolved path finds nothing.
    const repoDir = await fs.promises.realpath(rawRepoDir);

    const first = await waitFor(() => cachedRepo(repoDir));
    const activeBefore = repositoryCache.numberOfActiveServers();

    const {dispose: disposeSecond} = await openSecondClient(
      repoDir,
      first.initialConnectionContext.cmd,
    );

    try {
      // Same instance, not merely an equal one. A second Repository would mean a second `sl`
      // subscription, a second set of repo-global fetches, and a cold cache for the new window.
      // Compared as booleans on purpose: `toBe` on two Repository objects makes jest try to
      // deep-diff them on failure, which exhausts the heap and aborts the run instead of
      // reporting the mismatch.
      expect(cachedRepo(repoDir) === first).toBe(true);

      // NOTE: there is deliberately no `codeReviewProvider` assertion here. It would be dead
      // twice over: this synthetic repo has `paths.default=eager:...`, which is neither a
      // Mononoke nor a GitHub URL, so `codeReviewSystem` is `unknown` and the provider is never
      // constructed; and once the line above has established the two Repository objects are
      // identical, reading any property off both is a tautology. The caches those providers own
      // are shared *because* the Repository is — that is what the line above pins.

      // `numberOfActiveServers` sums *references* across repos. Exactly one more reference,
      // against the same instance above, is what distinguishes reuse from both possible
      // failures: a second Repository would leave the instance check failing, and a connection
      // that never resolved a repo at all would add no reference and make this check fail.
      expect(repositoryCache.numberOfActiveServers()).toBe(activeBefore + 1);
    } finally {
      disposeSecond();
      await act(cleanup);
    }
  });

  it('keeps the Repository alive for the remaining client when one disconnects', async () => {
    const {repoDir: rawRepoDir, cleanup} = await initRepo();
    const repoDir = await fs.promises.realpath(rawRepoDir);

    const first = await waitFor(() => cachedRepo(repoDir));

    const {dispose: disposeSecond} = await openSecondClient(
      repoDir,
      first.initialConnectionContext.cmd,
    );

    // Disposing the second client is a step of this test, but it also has to happen when an
    // assertion above it throws: this suite shares a process with every other integration file, so
    // a leaked connection or repo directory fails a later one instead of this one.
    let disposed = false;
    const disposeOnce = () => {
      if (!disposed) {
        disposed = true;
        disposeSecond();
      }
    };

    try {
      expect(cachedRepo(repoDir) === first).toBe(true);

      // Closing the second window must not take the first window's repo — and with it every warm
      // cache — down with it.
      disposeOnce();

      expect(cachedRepo(repoDir) === first).toBe(true);
    } finally {
      disposeOnce();
      await act(cleanup);
    }
  });
});
