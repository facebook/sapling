/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TestingEventBus} from './__mocks__/MessageBus';
import type {
  ClientToServerMessage,
  ClientToServerMessageWithPayload,
  CommitInfo,
  Hash,
  Result,
  ServerToClientMessage,
  SmartlogCommits,
  UncommittedChanges,
} from './types';
import type {Writable} from 'shared/typeUtils';

import messageBus from './MessageBus';
import {deserializeFromString, serializeToString} from './serialize';
import {mostRecentSubscriptionIds} from './serverAPIState';
import {screen, act, within} from '@testing-library/react';
import {selector, snapshot_UNSTABLE} from 'recoil';

const testMessageBus = messageBus as TestingEventBus;

export function simulateMessageFromServer(message: ServerToClientMessage): void {
  testMessageBus.simulateMessage(serializeToString(message));
}

export function expectMessageSentToServer(
  message: Partial<ClientToServerMessage | ClientToServerMessageWithPayload>,
): void {
  expect(
    testMessageBus.sent
      .filter((msg: unknown): msg is string => !(msg instanceof ArrayBuffer))
      .map(deserializeFromString),
  ).toContainEqual(message);
}
export function expectMessageNOTSentToServer(message: Partial<ClientToServerMessage>): void {
  expect(
    testMessageBus.sent
      .filter((msg: unknown): msg is string => !(msg instanceof ArrayBuffer))
      .map(deserializeFromString),
  ).not.toContainEqual(message);
}

/**
 * Return last `num` raw messages sent to the server.
 * Normal messages will be stingified JSON.
 * Binary messages with be ArrayBuffers.
 */
export function getLastMessagesSentToServer(num: number): Array<string | ArrayBuffer> {
  return testMessageBus.sent.slice(-num);
}

export function getLastBinaryMessageSentToServer(): ArrayBuffer | undefined {
  return testMessageBus.sent.find(
    (message): message is ArrayBuffer => message instanceof ArrayBuffer,
  );
}

export function simulateServerDisconnected(): void {
  testMessageBus.simulateServerStatusChange({type: 'error', error: 'server disconnected'});
}

export function simulateCommits(commits: Result<SmartlogCommits>) {
  simulateMessageFromServer({
    type: 'subscriptionResult',
    kind: 'smartlogCommits',
    subscriptionID: mostRecentSubscriptionIds.smartlogCommits,
    data: {
      fetchStartTimestamp: 1,
      fetchCompletedTimestamp: 2,
      commits,
    },
  });
}
export function simulateUncommittedChangedFiles(files: Result<UncommittedChanges>) {
  simulateMessageFromServer({
    type: 'subscriptionResult',
    kind: 'uncommittedChanges',
    subscriptionID: mostRecentSubscriptionIds.uncommittedChanges,
    data: {
      fetchStartTimestamp: 1,
      fetchCompletedTimestamp: 2,
      files,
    },
  });
}
export function simulateRepoConnected() {
  simulateMessageFromServer({
    type: 'repoInfo',
    info: {
      type: 'success',
      repoRoot: '/path/to/repo',
      dotdir: '/path/to/repo/.sl',
      command: 'sl',
      pullRequestDomain: undefined,
      codeReviewSystem: {type: 'github', owner: 'owner', repo: 'repo', hostname: 'github.com'},
    },
  });
}

export function resetTestMessages() {
  testMessageBus.resetTestMessages();
}

export function commitInfoIsOpen(): boolean {
  return (
    screen.queryByTestId('commit-info-view') != null ||
    screen.queryByTestId('commit-info-view-loading') != null
  );
}

export function closeCommitInfoSidebar() {
  if (!commitInfoIsOpen()) {
    return;
  }
  screen.queryAllByTestId('drawer-label').forEach(el => {
    const commitInfoTab = within(el).queryByText('Commit Info');
    commitInfoTab?.click();
  });
}

export function openCommitInfoSidebar() {
  if (commitInfoIsOpen()) {
    return;
  }
  screen.queryAllByTestId('drawer-label').forEach(el => {
    const commitInfoTab = within(el).queryByText('Commit Info');
    commitInfoTab?.click();
  });
}

export const emptyCommit: CommitInfo = {
  title: 'title',
  hash: '0',
  parents: [],
  phase: 'draft',
  isHead: false,
  author: 'author',
  date: new Date(),
  description: '',
  bookmarks: [],
  remoteBookmarks: [],
  filesSample: [],
  totalFileCount: 0,
};

export function COMMIT(
  hash: string,
  title: string,
  parent: Hash,
  info?: Partial<CommitInfo>,
): CommitInfo {
  return {
    ...emptyCommit,
    ...info,
    hash,
    title,
    parents: [parent],
  };
}

/**
 ```
    | z - Commit Z
    | |
    | y - Commit Y
    | |
    | x - Commit X
    |/
    2 - another public branch (remote/master)
    |
    | e - Commit E  (You are here)
    | |
    | d - Commit D
    | |
    | c - Commit C
    | |
    | b - Commit B
    | |
    | a - Commit A
    |/
    1 - some public base
    ```
*/
export const TEST_COMMIT_HISTORY = [
  COMMIT('z', 'Commit Z', 'y'),
  COMMIT('y', 'Commit Y', 'x'),
  COMMIT('x', 'Commit X', '2'),
  COMMIT('2', 'another public branch', '0', {phase: 'public', remoteBookmarks: ['remote/master']}),
  COMMIT('e', 'Commit E', 'd', {isHead: true}),
  COMMIT('d', 'Commit D', 'c'),
  COMMIT('c', 'Commit C', 'b'),
  COMMIT('b', 'Commit B', 'a'),
  COMMIT('a', 'Commit A', '1'),
  COMMIT('1', 'some public base', '0', {phase: 'public'}),
];

export const fireMouseEvent = function (
  type: string,
  elem: EventTarget,
  centerX: number,
  centerY: number,
  additionalProperties?: Partial<MouseEvent | InputEvent>,
) {
  const evt = document.createEvent('MouseEvents') as Writable<MouseEvent & InputEvent>;
  evt.initMouseEvent(
    type,
    true,
    true,
    window,
    1,
    1,
    1,
    centerX,
    centerY,
    false,
    false,
    false,
    false,
    0,
    elem,
  );
  evt.dataTransfer = {} as DataTransfer;
  if (additionalProperties != null) {
    for (const [key, value] of Object.entries(additionalProperties)) {
      (evt as Record<string, unknown>)[key] = value;
    }
  }
  return elem.dispatchEvent(evt);
};

// See https://github.com/testing-library/user-event/issues/440
export const dragAndDrop = (elemDrag: HTMLElement, elemDrop: HTMLElement) => {
  act(() => {
    // calculate positions
    let pos = elemDrag.getBoundingClientRect();
    const center1X = Math.floor((pos.left + pos.right) / 2);
    const center1Y = Math.floor((pos.top + pos.bottom) / 2);

    pos = elemDrop.getBoundingClientRect();
    const center2X = Math.floor((pos.left + pos.right) / 2);
    const center2Y = Math.floor((pos.top + pos.bottom) / 2);

    // mouse over dragged element and mousedown
    fireMouseEvent('mousemove', elemDrag, center1X, center1Y);
    fireMouseEvent('mouseenter', elemDrag, center1X, center1Y);
    fireMouseEvent('mouseover', elemDrag, center1X, center1Y);
    fireMouseEvent('mousedown', elemDrag, center1X, center1Y);

    if (!elemDrag.draggable) {
      return;
    }

    // start dragging process over to drop target
    const dragStarted = fireMouseEvent('dragstart', elemDrag, center1X, center1Y);
    if (!dragStarted) {
      return;
    }

    fireMouseEvent('drag', elemDrag, center1X, center1Y);
    fireMouseEvent('mousemove', elemDrag, center1X, center1Y);
    fireMouseEvent('drag', elemDrag, center2X, center2Y);
    fireMouseEvent('mousemove', elemDrop, center2X, center2Y);

    // trigger dragging process on top of drop target
    fireMouseEvent('mouseenter', elemDrop, center2X, center2Y);
    fireMouseEvent('dragenter', elemDrop, center2X, center2Y);
    fireMouseEvent('mouseover', elemDrop, center2X, center2Y);
    fireMouseEvent('dragover', elemDrop, center2X, center2Y);

    // release dragged element on top of drop target
    fireMouseEvent('drop', elemDrop, center2X, center2Y);
    fireMouseEvent('dragend', elemDrag, center2X, center2Y);
    fireMouseEvent('mouseup', elemDrag, center2X, center2Y);
  });
};

export function dragAndDropCommits(draggedCommit: Hash | HTMLElement, onto: Hash) {
  const draggableCommit =
    typeof draggedCommit !== 'string'
      ? draggedCommit
      : within(screen.getByTestId(`commit-${draggedCommit}`)).queryByTestId('draggable-commit');
  expect(draggableCommit).toBeDefined();
  const dragTargetComit = screen.queryByTestId(`commit-${onto}`)?.querySelector('.commit-details');
  expect(dragTargetComit).toBeDefined();

  act(() => {
    dragAndDrop(draggableCommit as HTMLElement, dragTargetComit as HTMLElement);
    jest.advanceTimersByTime(2);
  });
}

/**
 * Despite catching the error in our error boundary, react + jsdom still
 * print big scary messages to console.warn when we throw an error.
 * We can ignore these during the select tests that test error boundaries.
 * This should be done only when needed, to prevent filtering out useful
 * console.error statements.
 * See also: https://github.com/facebook/react/issues/11098#issuecomment-412682721
 */
export function suppressReactErrorBoundaryErrorMessages() {
  beforeAll(() => {
    jest.spyOn(console, 'error').mockImplementation(() => undefined);
  });
  afterAll(() => {
    jest.restoreAllMocks();
  });
}

const clearSelectorCachesState = selector({
  key: 'clearSelectorCachesState',
  get: ({getCallback}) =>
    getCallback(({snapshot, refresh}) => () => {
      for (const node of snapshot.getNodes_UNSTABLE()) {
        refresh(node);
      }
    }),
});

export const clearAllRecoilSelectorCaches = () => {
  snapshot_UNSTABLE().getLoadable(clearSelectorCachesState).getValue();
};
