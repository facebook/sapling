/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StackPullRequestFragment} from './generated/graphql';

import {useCommand} from './KeyboardShortcuts';
import PullRequestStackItem from './PullRequestStackItem';
import {gitHubClientAtom, gitHubPullRequestIDAtom, stackedPullRequestFragmentsAtom} from './jotai';
import {usePullRequestStackGraph} from './pullRequestStackGraph';
import useNavigateToPullRequest from './useNavigateToPullRequest';
import {ArrowDownIcon, ArrowUpIcon} from '@primer/octicons-react';
import {ActionList, ActionMenu, ButtonGroup, IconButton, Text} from '@primer/react';
import {useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useCallback, useEffect, useState} from 'react';

const loadableStackedPullRequestFragmentsAtom = loadable(stackedPullRequestFragmentsAtom);
const loadableGitHubClientAtom = loadable(gitHubClientAtom);
const STACK_MENU_ITEM_HEIGHT = 57;
const STACK_MENU_VERTICAL_PADDING = 16;
const MAX_VISIBLE_STACK_ITEMS = 12;
const MAX_STACK_MENU_HEIGHT =
  STACK_MENU_ITEM_HEIGHT * MAX_VISIBLE_STACK_ITEMS + STACK_MENU_VERTICAL_PADDING;

export default function PullRequestStack(): React.ReactElement | null {
  const navigateToPullRequest = useNavigateToPullRequest();
  const pullRequestNumber = useAtomValue(gitHubPullRequestIDAtom);

  // Keep the last path while navigation loads the next pull request. This
  // prevents the stack controls from disappearing between adjacent PRs.
  const stackLoadable = useAtomValue(loadableStackedPullRequestFragmentsAtom);
  const [lastStack, setLastStack] = useState<StackPullRequestFragment[] | null>(null);
  const availableStack = stackLoadable.state === 'hasData' ? stackLoadable.data : null;
  useEffect(() => {
    if (availableStack != null) {
      setLastStack(availableStack);
    }
  }, [availableStack, setLastStack]);

  const stack = availableStack ?? lastStack;
  const index = stack != null ? stack.findIndex(({number}) => number === pullRequestNumber) : -1;
  const clientLoadable = useAtomValue(loadableGitHubClientAtom);
  const client = clientLoadable.state === 'hasData' ? clientLoadable.data : null;
  const graphLoadable = usePullRequestStackGraph(client, pullRequestNumber, stack);
  const graph = graphLoadable.state === 'hasValue' ? graphLoadable.data : null;

  const onNavigate = useCallback(
    (index: number) => {
      if (stack == null || index === -1) {
        return;
      }

      const pullRequest = stack[index];
      if (pullRequest != null) {
        navigateToPullRequest(pullRequest.number);
      }
    },
    [navigateToPullRequest, stack],
  );

  useCommand('NextInStack', () => {
    if (stack == null || index === -1) {
      return;
    }
    if (index > 0) {
      onNavigate(index - 1);
    }
  });
  useCommand('PreviousInStack', () => {
    if (stack == null || index === -1) {
      return;
    }
    if (index < stack.length - 1) {
      onNavigate(index + 1);
    }
  });

  if (stack == null || index === -1 || stack.length === 0) {
    return null;
  }

  const total = stack.length;
  const hasPrev = index < total - 1;
  const hasNext = index > 0;
  const graphRows = graph?.rows;
  const graphCount = graphRows?.length ?? total;
  const buttonLabel = graph?.isBranched
    ? `Stack graph · ${graphCount} pull requests`
    : `Pull Request ${total - index} of ${total}`;

  return (
    <ButtonGroup>
      <ActionMenu>
        <ActionMenu.Button sx={{display: 'inline-block'}}>{buttonLabel}</ActionMenu.Button>
        <ActionMenu.Overlay
          width="xxlarge"
          sx={{
            maxHeight: `min(${MAX_STACK_MENU_HEIGHT}px, calc(100vh - 32px))`,
            overflowY: 'auto',
            zIndex: 100,
          }}>
          <ActionList selectionVariant="single">
            {graphRows != null
              ? graphRows.map(({graphPosition, pullRequest}) => (
                  <PullRequestStackItem
                    key={pullRequest.number}
                    graphPosition={graphPosition}
                    isSelected={pullRequestNumber === pullRequest.number}
                    {...pullRequest}
                  />
                ))
              : stack.map((pullRequest, stackIndex) => (
                  <PullRequestStackItem
                    key={pullRequest.number}
                    isSelected={index === stackIndex}
                    {...pullRequest}
                  />
                ))}
            {graphLoadable.state === 'hasError' && (
              <ActionList.Item disabled={true}>
                <Text color="danger.fg" fontSize={0}>
                  Stack graph unavailable: {graphLoadable.error.message}
                </Text>
              </ActionList.Item>
            )}
          </ActionList>
        </ActionMenu.Overlay>
      </ActionMenu>
      <IconButton disabled={!hasPrev} icon={ArrowDownIcon} onClick={() => onNavigate(index + 1)}>
        Prev
      </IconButton>
      <IconButton disabled={!hasNext} icon={ArrowUpIcon} onClick={() => onNavigate(index - 1)}>
        Next
      </IconButton>
    </ButtonGroup>
  );
}
