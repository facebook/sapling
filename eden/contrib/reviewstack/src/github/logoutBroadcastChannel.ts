/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/*
 * This file contains the logic that coordinates logout activity across multiple
 * instances of ReviewStack opened within the same web browser. It leverages a
 * BroadcastChannel to notify all of the relevant windows and SharedWorkers
 * (e.g., "browsing contexts") that the user has opted to logout, which means
 * that:
 *
 * - The context should close any open connection to IndexedDB that it has.
 * - The context should not do any more writes to the IndexedDB (which should
 *   follow from the previous bullet point).
 * - The context in which the user initiated the logout should take
 *   responsibility for calling `clearAllLocalData()` in gitHubCredentials.ts.
 *   In order for the `indexedDB.deleteDatabase()` call to succeed, all of the
 *   other contexts will have had to close their database connections first.
 */

const LOGOUT_CHANNEL_NAME = 'reviewstack-logout';
const INTERNAL_LOGOUT_EVENT_NAME = 'reviewstack-logout-this-window';

const logoutBroadcastChannel = new BroadcastChannel(LOGOUT_CHANNEL_NAME);

type LogoutMessage = {
  logout: true;
};

/**
 * Used to register listeners via subscribeToLogout() for clients that also
 * want to be notified of logout events from the source window.
 *
 * We use this EventTarget with the INTERNAL_LOGOUT_EVENT_NAME Event that is
 * private to this module.
 */
const localSubscribers = new EventTarget();

/**
 * Calls the specified callback when a "logout" event is received on the
 * channel. By default, the callback will only be called when the message was
 * fired from another browser tab.
 *
 * Returns a function to remove the subscriptions.
 */
export function subscribeToLogout(
  callback: () => void,
  includeLogoutEventsFromThisWindow = false,
): () => void {
  const unsubscribeCalls: Array<() => void> = [];

  const channelListener = ({data}: MessageEvent) => {
    if ((data as LogoutMessage).logout === true) {
      callback();
    }
  };
  logoutBroadcastChannel.addEventListener('message', channelListener);
  unsubscribeCalls.push(() =>
    logoutBroadcastChannel.removeEventListener('message', channelListener),
  );

  if (includeLogoutEventsFromThisWindow) {
    // Ensure the original callback does not see the Event.
    const localSubscribersListener = (_event: Event) => {
      callback();
    };
    localSubscribers.addEventListener(INTERNAL_LOGOUT_EVENT_NAME, localSubscribersListener);
    unsubscribeCalls.push(() =>
      localSubscribers.removeEventListener(INTERNAL_LOGOUT_EVENT_NAME, localSubscribersListener),
    );
  }

  return () => unsubscribeCalls.forEach(callback => callback());
}

export function broadcastLogoutMessage() {
  localSubscribers.dispatchEvent(new Event(INTERNAL_LOGOUT_EVENT_NAME));
  const message: LogoutMessage = {logout: true};
  logoutBroadcastChannel.postMessage(message);
}
