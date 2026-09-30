/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PullRequest} from './github/pullRequestTimelineTypes';

import {
  gitHubClientAtom,
  notificationMessageAtom,
  stackedPullRequestFragmentsAtom,
} from './jotai';
import {buildPullRequestMergePlan, mergePullRequestStack} from './pullRequestMerge';
import useRefreshPullRequest from './useRefreshPullRequest';
import {Button, Tooltip} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {loadable} from 'jotai/utils';
import {useMemo, useState} from 'react';

const loadableStackedPullRequestFragmentsAtom = loadable(stackedPullRequestFragmentsAtom);

export default function PullRequestMergeButton({
  pullRequest,
}: {
  pullRequest: PullRequest;
}): React.ReactElement | null {
  const client = useAtomValue(gitHubClientAtom);
  const stackLoadable = useAtomValue(loadableStackedPullRequestFragmentsAtom);
  const setNotification = useSetAtom(notificationMessageAtom);
  const refreshPullRequest = useRefreshPullRequest();
  const [checking, setChecking] = useState(false);
  const [progress, setProgress] = useState<{completed: number; total: number} | null>(null);

  const stack = useMemo(() => {
    if (stackLoadable.state !== 'hasData') {
      return null;
    }
    const loadedStack = stackLoadable.data;
    return loadedStack.some(({number}) => number === pullRequest.number)
      ? loadedStack
      : [pullRequest];
  }, [pullRequest, stackLoadable]);
  const plan = useMemo(
    () => (stack == null ? null : buildPullRequestMergePlan(stack, pullRequest.number)),
    [pullRequest.number, stack],
  );

  if (pullRequest.state !== 'OPEN') {
    return null;
  }

  const stackError =
    stackLoadable.state === 'hasError'
      ? stackLoadable.error instanceof Error
        ? stackLoadable.error.message
        : String(stackLoadable.error)
      : null;
  const disabledReason =
    stackLoadable.state === 'loading'
      ? 'Loading the Sapling stack.'
      : stackLoadable.state === 'hasError'
        ? `Could not load the Sapling stack: ${stackError}`
        : plan != null && plan.blockers.length > 0
          ? plan.blockers.join(' ')
          : plan?.pullRequests.length === 0
            ? 'There are no open pull requests to merge.'
            : null;
  const disabled = client == null || checking || progress != null || disabledReason != null;
  const count = plan?.totalPullRequests ?? 0;
  const label =
    checking
      ? 'Checking stack…'
      : progress == null
      ? count > 1
        ? `Merge ${count} commits to main`
        : 'Merge to main'
      : `Merging ${Math.min(progress.completed + 1, progress.total)} of ${progress.total}…`;

  const merge = async (): Promise<void> => {
    if (client == null || plan == null || disabled || plan.pullRequests.length === 0) {
      return;
    }
    let completed = 0;
    let freshPlan = plan;
    setChecking(true);
    try {
      const selectedIndex = stack?.findIndex(({number}) => number === pullRequest.number) ?? -1;
      const expectedNumbers = (stack ?? []).slice(selectedIndex).map(({number}) => number);
      const freshStack = await client.getFreshStackPullRequests(expectedNumbers);
      const freshNumbers = new Set(freshStack.map(({number}) => number));
      const missingNumbers = expectedNumbers.filter(number => !freshNumbers.has(number));
      if (missingNumbers.length > 0) {
        throw new Error(`GitHub did not return ${missingNumbers.map(number => `#${number}`).join(', ')}.`);
      }
      freshPlan = buildPullRequestMergePlan(freshStack, pullRequest.number);
      if (freshPlan.blockers.length > 0 || freshPlan.pullRequests.length === 0) {
        throw new Error(
          freshPlan.blockers.join(' ') || 'There are no open pull requests to merge.',
        );
      }
      const lines = freshPlan.pullRequests
        .map(({number, title}) => `#${number} ${title}`)
        .join('\n');
      const confirmed = window.confirm(
        `Squash-merge ${freshPlan.pullRequests.length} approved pull request${
          freshPlan.pullRequests.length === 1 ? '' : 's'
        } into main, oldest first?\n\n${lines}\n\nIf one merge fails, later pull requests will not be merged.`,
      );
      if (!confirmed) {
        return;
      }

      setChecking(false);
      setProgress({completed, total: freshPlan.pullRequests.length});
      await mergePullRequestStack(client, freshPlan.pullRequests, nextCompleted => {
        completed = nextCompleted;
        setProgress({completed, total: freshPlan.pullRequests.length});
      });
      setNotification({
        type: 'info',
        message: `Merged ${freshPlan.pullRequests.length} pull request${
          freshPlan.pullRequests.length === 1 ? '' : 's'
        } into main.`,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setNotification({
        type: 'error',
        message: `Merged ${completed} of ${freshPlan.pullRequests.length}. Stopped: ${message}`,
      });
    } finally {
      setChecking(false);
      setProgress(null);
      refreshPullRequest();
    }
  };

  const button = (
    <Button disabled={disabled} onClick={merge} variant="primary">
      {label}
    </Button>
  );
  return disabledReason == null ? button : <Tooltip text={disabledReason}>{button}</Tooltip>;
}
