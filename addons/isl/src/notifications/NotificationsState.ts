/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Notification} from '../types';

import {atom} from 'jotai';
import serverAPI from '../ClientToServerAPI';
import {localStorageBackedAtom, readAtom, writeAtom} from '../jotaiUtils';
import {pageVisibility} from '../codeReview/CodeReviewInfo';
import {registerCleanup, registerDisposable} from '../utils';
import {repositoryInfo} from '../serverAPIState';

// Polling intervals
const FOCUSED_POLL_INTERVAL = 2 * 60 * 1000; // 2 minutes
const BACKGROUND_POLL_INTERVAL = 5 * 60 * 1000; // 5 minutes

// Raw notifications from server
export const notificationsAtom = atom<Notification[]>([]);

// Dismissed notification IDs stored in local storage
export const dismissedNotificationIdsAtom = localStorageBackedAtom<string[]>(
  'isl.dismissed-notification-ids',
  [],
);

// Filtered notifications (excluding dismissed)
export const activeNotificationsAtom = atom(get => {
  const notifications = get(notificationsAtom);
  const dismissedIds = new Set(get(dismissedNotificationIdsAtom));
  return notifications.filter(n => !dismissedIds.has(n.id));
});

// Notification count for badge
export const notificationCountAtom = atom(get => {
  return get(activeNotificationsAtom).length;
});

// Loading state
export const isLoadingNotificationsAtom = atom(false);

// Error state
export const notificationsErrorAtom = atom<Error | null>(null);

// Fetch notifications from server
export function fetchNotifications(): void {
  const repoInfo = readAtom(repositoryInfo);
  // Only fetch for GitHub repos
  if (repoInfo?.codeReviewSystem.type !== 'github') {
    return;
  }

  writeAtom(isLoadingNotificationsAtom, true);
  serverAPI.postMessage({type: 'fetchNotifications'});
}

// Dismiss a notification
export function dismissNotification(notificationId: string): void {
  const currentDismissed = readAtom(dismissedNotificationIdsAtom);
  if (!currentDismissed.includes(notificationId)) {
    writeAtom(dismissedNotificationIdsAtom, [...currentDismissed, notificationId]);
  }
}

// Clear all dismissed notifications (for cleanup)
export function clearDismissedNotifications(): void {
  writeAtom(dismissedNotificationIdsAtom, []);
}

// Handle incoming notification data
registerDisposable(
  notificationsAtom,
  serverAPI.onMessageOfType('fetchedNotifications', event => {
    writeAtom(isLoadingNotificationsAtom, false);
    if (event.notifications.value) {
      writeAtom(notificationsAtom, event.notifications.value);
      writeAtom(notificationsErrorAtom, null);
    } else if (event.notifications.error) {
      writeAtom(notificationsErrorAtom, event.notifications.error);
    }
  }),
  import.meta.hot,
);

// Polling logic
let pollIntervalId: ReturnType<typeof setTimeout> | null = null;

function startPolling(): void {
  if (pollIntervalId != null) {
    return;
  }

  const poll = () => {
    const visibility = readAtom(pageVisibility);
    fetchNotifications();

    // Adjust interval based on visibility
    const interval = visibility === 'focused' ? FOCUSED_POLL_INTERVAL : BACKGROUND_POLL_INTERVAL;

    pollIntervalId = setTimeout(() => {
      pollIntervalId = null;
      poll();
    }, interval);
  };

  // Initial fetch
  poll();
}

function stopPolling(): void {
  if (pollIntervalId != null) {
    clearTimeout(pollIntervalId);
    pollIntervalId = null;
  }
}

// Start polling when server connection is ready
registerCleanup(
  notificationsAtom,
  serverAPI.onSetup(() => {
    startPolling();
    return () => stopPolling();
  }),
  import.meta.hot,
);

// React to visibility changes to adjust polling
registerDisposable(
  notificationsAtom,
  serverAPI.onMessageOfType('repoInfo', () => {
    // Refetch when repo changes
    fetchNotifications();
  }),
  import.meta.hot,
);
