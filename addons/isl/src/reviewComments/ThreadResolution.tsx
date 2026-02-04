/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useState} from 'react';
import serverAPI from '../ClientToServerAPI';
import {t} from '../i18n';
import {showToast} from '../toast';

export type ThreadResolutionButtonProps = {
  threadId: string;
  isResolved: boolean;
  onStatusChange?: (newStatus: boolean) => void;
};

/**
 * Button component for resolving/unresolving comment threads.
 * Sends message to server which executes GraphQL mutation on GitHub.
 */
export function ThreadResolutionButton({
  threadId,
  isResolved,
  onStatusChange,
}: ThreadResolutionButtonProps) {
  const [isLoading, setIsLoading] = useState(false);

  const handleClick = async () => {
    setIsLoading(true);
    try {
      const messageType = isResolved ? 'unresolveThread' : 'resolveThread';
      serverAPI.postMessage({
        type: messageType,
        threadId,
      });

      const result = await serverAPI.nextMessageMatching(
        'threadResolutionResult',
        msg => msg.threadId === threadId,
      );

      if (result.error) {
        throw new Error(result.error);
      }

      onStatusChange?.(!isResolved);
    } catch (error) {
      const action = isResolved ? 'unresolve' : 'resolve';
      showToast(t(`Failed to ${action} thread`), {durationMs: 5000});
    } finally {
      setIsLoading(false);
    }
  };

  const buttonLabel = isResolved ? t('Unresolve') : t('Resolve');
  const tooltipText = isResolved
    ? t('Mark thread as unresolved')
    : t('Mark thread as resolved');
  const iconName = isResolved ? 'circle-large-outline' : 'check';

  return (
    <Tooltip title={tooltipText}>
      <Button icon disabled={isLoading} onClick={handleClick}>
        {isLoading ? <Icon icon="loading" /> : <Icon icon={iconName} />}
        {buttonLabel}
      </Button>
    </Tooltip>
  );
}
