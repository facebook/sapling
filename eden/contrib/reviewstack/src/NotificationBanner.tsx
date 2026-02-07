/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {NotificationMessage} from './jotai/atoms';

import {notificationMessageAtom} from './jotai/atoms';
import {XIcon} from '@primer/octicons-react';
import {Box, Flash, IconButton} from '@primer/react';
import {useAtom} from 'jotai';
import {useCallback, useEffect} from 'react';

/**
 * A banner that displays notification messages to the user.
 * Notifications auto-dismiss after 5 seconds or can be dismissed manually.
 */
export default function NotificationBanner(): React.ReactElement | null {
  const [notification, setNotification] = useAtom(notificationMessageAtom);

  const dismiss = useCallback(() => {
    setNotification(null);
  }, [setNotification]);

  // Auto-dismiss after 5 seconds
  useEffect(() => {
    if (notification != null) {
      const timeout = setTimeout(dismiss, 5000);
      return () => clearTimeout(timeout);
    }
  }, [notification, dismiss]);

  if (notification == null) {
    return null;
  }

  const variant = getFlashVariant(notification.type);

  return (
    <Box
      position="fixed"
      bottom={3}
      left="50%"
      sx={{
        transform: 'translateX(-50%)',
        zIndex: 1000,
        maxWidth: '600px',
        width: '90%',
      }}>
      <Flash variant={variant}>
        <Box display="flex" alignItems="center" justifyContent="space-between">
          <Box flex={1}>{notification.message}</Box>
          <IconButton
            icon={XIcon}
            aria-label="Dismiss"
            variant="invisible"
            onClick={dismiss}
            sx={{marginLeft: 2}}
          />
        </Box>
      </Flash>
    </Box>
  );
}

function getFlashVariant(
  type: NonNullable<NotificationMessage>['type'],
): 'default' | 'warning' | 'danger' {
  switch (type) {
    case 'info':
      return 'default';
    case 'warning':
      return 'warning';
    case 'error':
      return 'danger';
  }
}
