/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StackPullRequestFragment} from './generated/graphql';
import type GitHubClient from './github/GitHubClient';
import type {PullsPullRequest} from './github/pullsTypes';

import {PullRequestState} from './generated/graphql';
import {pullRequestNumbersFromBody} from './ghstackUtils';
import {parseSaplingStackBody} from './saplingStack';
import {useEffect, useState} from 'react';

const PAGE_SIZE = 100;
const MAX_PULL_REQUESTS_TO_SCAN = 500;

export type PullRequestStackGraphRow = {
  graphPosition: StackGraphPosition;
  pullRequest: StackPullRequestFragment;
};

export type StackGraphPosition = {
  incomingLanes: number[];
  nodeLane: number;
  outgoing: boolean;
  throughLanes: number[];
};

export type PullRequestStackGraph = {
  isBranched: boolean;
  rows: PullRequestStackGraphRow[];
};

export type PullRequestStackGraphLoadable =
  | {state: 'loading'}
  | {state: 'hasValue'; data: PullRequestStackGraph | null}
  | {state: 'hasError'; error: Error};

type StackBody = {
  body: string;
};

type StackTopologyRow = {
  graphPosition: StackGraphPosition;
  number: number;
};

type StackTopology = {
  isBranched: boolean;
  rows: StackTopologyRow[];
};

export function stackPathFromBody(body: string): number[] {
  const saplingStack = parseSaplingStackBody(body);
  if (saplingStack != null) {
    return saplingStack.stack.map(({number}) => number);
  }
  return pullRequestNumbersFromBody(body) ?? [];
}

/**
 * Combine every stack path that overlaps the current path. Sapling writes each
 * path from its newest PR to its oldest PR, so adjacent entries define a child
 * followed by its parent. The union reveals siblings that share a parent.
 */
export function buildStackTopology(
  currentPullRequest: number,
  currentPath: number[],
  pullRequests: StackBody[],
  availablePullRequests?: ReadonlySet<number>,
): StackTopology {
  const filterAvailable = (path: number[]): number[] =>
    availablePullRequests == null
      ? path
      : path.filter(number => availablePullRequests.has(number));
  const availableCurrentPath = filterAvailable(currentPath);
  const paths = [
    availableCurrentPath,
    ...pullRequests.map(({body}) => filterAvailable(stackPathFromBody(body))),
  ].filter(path => path.length > 0);
  const connected = new Set(
    availableCurrentPath.length > 0 ? availableCurrentPath : [currentPullRequest],
  );
  connected.add(currentPullRequest);

  let changed = true;
  const connectedPaths = new Set<number>();
  while (changed) {
    changed = false;
    paths.forEach((path, pathIndex) => {
      if (connectedPaths.has(pathIndex) || !path.some(number => connected.has(number))) {
        return;
      }
      connectedPaths.add(pathIndex);
      path.forEach(number => {
        if (!connected.has(number)) {
          connected.add(number);
          changed = true;
        }
      });
    });
  }

  const parentByChild = new Map<number, number>();
  paths.forEach((path, pathIndex) => {
    if (!connectedPaths.has(pathIndex)) {
      return;
    }
    for (let index = 0; index < path.length - 1; ++index) {
      const child = path[index];
      const parent = path[index + 1];
      if (connected.has(child) && connected.has(parent) && !parentByChild.has(child)) {
        parentByChild.set(child, parent);
      }
    }
  });

  const childrenByParent = new Map<number, number[]>();
  parentByChild.forEach((parent, child) => {
    const children = childrenByParent.get(parent) ?? [];
    children.push(child);
    childrenByParent.set(parent, children);
  });
  childrenByParent.forEach(children => children.sort((a, b) => a - b));

  const roots = [...connected]
    .filter(number => !connected.has(parentByChild.get(number) ?? -1))
    .sort((a, b) => a - b);
  const preferredChildByParent = new Map<number, number>();
  for (let index = 0; index < availableCurrentPath.length - 1; ++index) {
    preferredChildByParent.set(availableCurrentPath[index + 1], availableCurrentPath[index]);
  }

  const orderedChildren = (number: number): number[] => {
    const children = childrenByParent.get(number) ?? [];
    const preferred = preferredChildByParent.get(number);
    return [...children].sort((left, right) => {
      if (left === preferred) {
        return -1;
      }
      if (right === preferred) {
        return 1;
      }
      return right - left;
    });
  };

  const laneByNumber = new Map<number, number>();
  let nextLane = roots.length;
  const visited = new Set<number>();
  const assignLanes = (number: number, lane: number): void => {
    if (visited.has(number)) {
      return;
    }
    visited.add(number);
    laneByNumber.set(number, lane);
    orderedChildren(number).forEach((child, index) => {
      const childLane = index === 0 ? lane : nextLane++;
      assignLanes(child, childLane);
    });
  };
  roots.forEach((root, index) => assignLanes(root, index));
  [...connected].sort((a, b) => a - b).forEach(number => assignLanes(number, nextLane++));

  const orderedNumbers: number[] = [];
  visited.clear();
  const visitChildrenFirst = (number: number): void => {
    if (visited.has(number)) {
      return;
    }
    visited.add(number);
    orderedChildren(number).forEach(visitChildrenFirst);
    orderedNumbers.push(number);
  };
  roots.forEach(visitChildrenFirst);
  [...connected].sort((a, b) => a - b).forEach(visitChildrenFirst);

  const rowByNumber = new Map(orderedNumbers.map((number, index) => [number, index]));
  const throughLanesByNumber = new Map<number, Set<number>>();
  parentByChild.forEach((parent, child) => {
    const childRow = rowByNumber.get(child);
    const parentRow = rowByNumber.get(parent);
    const lane = laneByNumber.get(child);
    if (childRow == null || parentRow == null || lane == null) {
      return;
    }
    for (let row = childRow + 1; row < parentRow; ++row) {
      const number = orderedNumbers[row];
      const throughLanes = throughLanesByNumber.get(number) ?? new Set<number>();
      throughLanes.add(lane);
      throughLanesByNumber.set(number, throughLanes);
    }
  });

  const rows = orderedNumbers.map(number => ({
    graphPosition: {
      incomingLanes: orderedChildren(number)
        .map(child => laneByNumber.get(child))
        .filter((lane): lane is number => lane != null),
      nodeLane: laneByNumber.get(number) ?? 0,
      outgoing: parentByChild.has(number),
      throughLanes: [...(throughLanesByNumber.get(number) ?? [])].sort((a, b) => a - b),
    },
    number,
  }));

  return {
    isBranched: [...childrenByParent.values()].some(children => children.length > 1),
    rows,
  };
}

async function loadOpenPullRequests(client: GitHubClient): Promise<PullsPullRequest[]> {
  const pullRequests: PullsPullRequest[] = [];
  let after: string | null | undefined;
  let hasNextPage = true;
  while (hasNextPage && pullRequests.length < MAX_PULL_REQUESTS_TO_SCAN) {
    // Pagination is sequential because each request returns the next cursor.
    // eslint-disable-next-line no-await-in-loop
    const result = await client.getPullRequests({
      after,
      first: PAGE_SIZE,
      includeBody: true,
      labels: [],
      states: [PullRequestState.Open],
    });
    if (result == null) {
      throw new Error('GitHub did not return the repository pull requests.');
    }
    pullRequests.push(...result.pullRequests);
    hasNextPage = result.pageInfo.hasNextPage;
    after = result.pageInfo.endCursor;
    if (hasNextPage && after == null) {
      throw new Error('GitHub returned an incomplete page without a cursor.');
    }
  }
  if (hasNextPage) {
    throw new Error(
      `The repository has more than ${MAX_PULL_REQUESTS_TO_SCAN} open pull requests. ` +
        'Narrow stack graph discovery before rendering this repository.',
    );
  }
  return pullRequests;
}

export function usePullRequestStackGraph(
  client: GitHubClient | null,
  currentPullRequest: number | null,
  currentStack: StackPullRequestFragment[] | null,
): PullRequestStackGraphLoadable {
  const stackKey = currentStack?.map(({number}) => number).join(',') ?? '';
  const [loadable, setLoadable] = useState<PullRequestStackGraphLoadable>({state: 'loading'});

  useEffect(() => {
    let cancelled = false;
    if (
      client == null ||
      currentPullRequest == null ||
      currentStack == null ||
      currentStack.length === 0
    ) {
      setLoadable({state: 'hasValue', data: null});
      return () => {
        cancelled = true;
      };
    }

    setLoadable({state: 'loading'});
    const load = async (): Promise<void> => {
      const openPullRequests = await loadOpenPullRequests(client);
      let topology = buildStackTopology(
        currentPullRequest,
        currentStack.map(({number}) => number),
        openPullRequests.filter(
          (pullRequest): pullRequest is PullsPullRequest & StackBody =>
            typeof pullRequest.body === 'string',
        ),
      );
      const fragments = await client.getStackPullRequests(topology.rows.map(({number}) => number));
      const fragmentsByNumber = new Map(fragments.map(fragment => [fragment.number, fragment]));
      if (fragments.length !== topology.rows.length) {
        topology = buildStackTopology(
          currentPullRequest,
          currentStack.map(({number}) => number),
          openPullRequests.filter(
            (pullRequest): pullRequest is PullsPullRequest & StackBody =>
              typeof pullRequest.body === 'string',
          ),
          new Set(fragmentsByNumber.keys()),
        );
      }
      const rows = topology.rows.flatMap(({graphPosition, number}) => {
        const pullRequest = fragmentsByNumber.get(number);
        return pullRequest == null ? [] : [{graphPosition, pullRequest}];
      });
      if (!cancelled) {
        setLoadable({state: 'hasValue', data: {isBranched: topology.isBranched, rows}});
      }
    };

    load().catch(error => {
      if (!cancelled) {
        setLoadable({
          state: 'hasError',
          error: error instanceof Error ? error : new Error(String(error)),
        });
      }
    });
    return () => {
      cancelled = true;
    };
  }, [client, currentPullRequest, currentStack, stackKey]);

  return loadable;
}
