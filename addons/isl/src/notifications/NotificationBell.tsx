/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Notification, NotificationType} from '../types';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {T} from '../i18n';
import platform from '../platform';
import {repositoryInfo} from '../serverAPIState';
import {
  activeNotificationsAtom,
  dismissNotification,
  isLoadingNotificationsAtom,
  notificationCountAtom,
} from './NotificationsState';

import './NotificationBell.css';

export function NotificationBell() {
  const repoInfo = useAtomValue(repositoryInfo);

  // Only show for GitHub repos
  if (repoInfo?.codeReviewSystem.type !== 'github') {
    return null;
  }

  return (
    <Tooltip
      trigger="click"
      component={dismiss => <NotificationDropdown dismiss={dismiss} />}
      placement="bottom"
      group="topbar"
      title={<T>GitHub Notifications</T>}>
      <Button icon data-testid="notification-bell-button" className="notification-bell-button">
        <NotificationBellIcon />
      </Button>
    </Tooltip>
  );
}

function NotificationBellIcon() {
  const count = useAtomValue(notificationCountAtom);
  const isLoading = useAtomValue(isLoadingNotificationsAtom);

  return (
    <span className="notification-bell-icon">
      <Icon icon={isLoading ? 'loading' : 'bell'} />
      {count > 0 && <span className="notification-badge">{count > 99 ? '99+' : count}</span>}
    </span>
  );
}

function NotificationDropdown({dismiss}: {dismiss: () => void}) {
  const notifications = useAtomValue(activeNotificationsAtom);
  const isLoading = useAtomValue(isLoadingNotificationsAtom);

  if (isLoading && notifications.length === 0) {
    return (
      <div className="notification-dropdown">
        <div className="notification-dropdown-loading">
          <Icon icon="loading" />
          <T>Loading notifications...</T>
        </div>
      </div>
    );
  }

  if (notifications.length === 0) {
    return (
      <div className="notification-dropdown">
        <div className="notification-dropdown-empty">
          <Icon icon="check" />
          <T>No notifications</T>
        </div>
      </div>
    );
  }

  return (
    <div className="notification-dropdown">
      <div className="notification-dropdown-header">
        <T>Notifications</T>
        <span className="notification-count">({notifications.length})</span>
      </div>
      <div className="notification-list">
        {notifications.map(notification => (
          <NotificationItem
            key={notification.id}
            notification={notification}
            onDismiss={() => dismissNotification(notification.id)}
            onView={() => {
              platform.openExternalLink(notification.prUrl);
              dismiss();
            }}
          />
        ))}
      </div>
    </div>
  );
}

function NotificationItem({
  notification,
  onDismiss,
  onView,
}: {
  notification: Notification;
  onDismiss: () => void;
  onView: () => void;
}) {
  const typeIcon = getNotificationTypeIcon(notification.type);
  const typeLabel = getNotificationTypeLabel(notification.type, notification.reviewState);

  return (
    <div className="notification-item">
      <div className="notification-item-content" onClick={onView}>
        {notification.actorAvatarUrl && (
          <img
            src={notification.actorAvatarUrl}
            alt={notification.actor}
            className="notification-avatar"
          />
        )}
        <div className="notification-item-details">
          <div className="notification-item-header">
            <Icon icon={typeIcon} className={`notification-type-icon ${notification.type}`} />
            <span className="notification-type-label">{typeLabel}</span>
          </div>
          <div className="notification-item-title">#{notification.prNumber} {notification.prTitle}</div>
          <div className="notification-item-meta">
            <span className="notification-actor">{notification.actor}</span>
            <span className="notification-timestamp">{formatTimestamp(notification.timestamp)}</span>
          </div>
        </div>
      </div>
      <Button
        icon
        className="notification-dismiss-button"
        onClick={e => {
          e.stopPropagation();
          onDismiss();
        }}
        data-testid="dismiss-notification">
        <Icon icon="x" />
      </Button>
    </div>
  );
}

function getNotificationTypeIcon(type: NotificationType): string {
  switch (type) {
    case 'review-request':
      return 'git-pull-request';
    case 'mention':
      return 'mention';
    case 'review-received':
      return 'comment-discussion';
    default:
      return 'bell';
  }
}

function getNotificationTypeLabel(
  type: NotificationType,
  reviewState?: Notification['reviewState'],
): string {
  switch (type) {
    case 'review-request':
      return 'Review Requested';
    case 'mention':
      return 'Mentioned';
    case 'review-received':
      switch (reviewState) {
        case 'APPROVED':
          return 'Approved';
        case 'CHANGES_REQUESTED':
          return 'Changes Requested';
        case 'COMMENTED':
          return 'Reviewed';
        default:
          return 'Review';
      }
    default:
      return 'Notification';
  }
}

function formatTimestamp(date: Date): string {
  const now = new Date();
  const diffMs = now.getTime() - new Date(date).getTime();
  const diffMins = Math.floor(diffMs / (1000 * 60));
  const diffHours = Math.floor(diffMs / (1000 * 60 * 60));
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

  if (diffMins < 1) {
    return 'just now';
  } else if (diffMins < 60) {
    return `${diffMins}m ago`;
  } else if (diffHours < 24) {
    return `${diffHours}h ago`;
  } else if (diffDays < 7) {
    return `${diffDays}d ago`;
  } else {
    return new Date(date).toLocaleDateString();
  }
}
