/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UserFragment} from './generated/graphql';
import type {ChangeEvent, KeyboardEvent, SyntheticEvent} from 'react';

import './PullRequestCommentInput.css';

import {getActiveMention, replaceActiveMention} from './commentMentions';
import {gitHubRepoMentionableUsersAtom, pendingScrollRestoreAtom} from './jotai';
import {capturePullRequestScrollPosition} from './pullRequestScroll';
import {Avatar, Box, Button, Flash, Textarea} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {loadable} from 'jotai/utils';
import {useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState} from 'react';
import {createPortal} from 'react-dom';

type Props = {
  /**
   * Returning a rejected Promise indicates the user should be allowed to try
   * to submit the form again.
   */
  addComment: (comment: string) => Promise<void>;
  /**
   * true if the component should still be rendered after the comment is added
   * successfully; false if the component is expected to be unmounted after the
   * comment is added successfully.
   */
  resetInputAfterAddingComment: boolean;
  autoFocus: boolean;
  onCancel?: () => void;
  allowEmptyMessage?: boolean;
  label?: string;
  actionSelector?: React.ReactNode;
  enableSuggestedChange?: boolean;
  suggestedChangeText?: string;
  initialComment?: string;
};

/**
 * Convert API error messages to user-friendly messages.
 */
function formatErrorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);

  // Handle specific GitHub API errors with user-friendly messages
  if (message.includes('end commit oid is not part of the pull request')) {
    return 'Cannot add comment: the commit you are viewing is no longer part of this pull request. This can happen after a force push. Try refreshing and viewing the latest version.';
  }

  if (message.includes('client not found')) {
    return 'Cannot add comment: not connected to GitHub. Please try refreshing the page.';
  }

  if (message.includes('pull request not found') || message.includes('pull request id not found')) {
    return 'Cannot add comment: pull request not found. Please try refreshing the page.';
  }

  return `Failed to add comment: ${message}`;
}

const CARET_STYLE_PROPERTIES = [
  'border-left-width',
  'border-right-width',
  'box-sizing',
  'font-family',
  'font-size',
  'font-style',
  'font-weight',
  'letter-spacing',
  'line-height',
  'padding-left',
  'padding-right',
  'padding-top',
  'tab-size',
  'text-align',
  'text-indent',
  'text-transform',
  'width',
  'word-spacing',
] as const;

function getTextareaCaretPosition(textarea: HTMLTextAreaElement): {left: number; top: number} {
  const computedStyle = window.getComputedStyle(textarea);
  const mirror = document.createElement('div');
  mirror.style.position = 'absolute';
  mirror.style.top = '0';
  mirror.style.left = '-9999px';
  mirror.style.visibility = 'hidden';
  mirror.style.whiteSpace = 'pre-wrap';
  mirror.style.overflowWrap = 'break-word';
  for (const property of CARET_STYLE_PROPERTIES) {
    mirror.style.setProperty(property, computedStyle.getPropertyValue(property));
  }

  mirror.textContent = textarea.value.slice(0, textarea.selectionStart);
  if (mirror.textContent.endsWith('\n')) {
    mirror.textContent += '\u200b';
  }
  const marker = document.createElement('span');
  marker.textContent = '\u200b';
  mirror.appendChild(marker);
  document.body.appendChild(mirror);

  const position = {
    left:
      marker.offsetLeft + parseFloat(computedStyle.borderLeftWidth) - textarea.scrollLeft,
    top: marker.offsetTop + parseFloat(computedStyle.borderTopWidth) - textarea.scrollTop,
  };
  mirror.remove();
  return position;
}

export default function PullRequestCommentInput({
  addComment,
  resetInputAfterAddingComment,
  autoFocus,
  onCancel,
  allowEmptyMessage = false,
  label = 'Add Comment',
  actionSelector,
  enableSuggestedChange = false,
  suggestedChangeText,
  initialComment = '',
}: Props): React.ReactElement {
  const [comment, setComment] = useState<string>(initialComment);
  const [disabled, setDisabled] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const setPendingScrollRestore = useSetAtom(pendingScrollRestoreAtom);
  const [activeMention, setActiveMention] = useState<ReturnType<typeof getActiveMention>>(null);
  const [mentionQuery, setMentionQuery] = useState<string | null>(null);
  const [selectedMentionIndex, setSelectedMentionIndex] = useState(0);
  const [mentionMenuDismissed, setMentionMenuDismissed] = useState(false);
  const [mentionMenuPosition, setMentionMenuPosition] = useState({left: 0, top: 0});
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const mentionMenuRef = useRef<HTMLDivElement>(null);
  const mentionMenuID = useId();

  useEffect(() => {
    if (activeMention == null || mentionMenuDismissed) {
      setMentionQuery(null);
      return;
    }
    if (activeMention.query === '') {
      setMentionQuery('');
      return;
    }
    const timeout = window.setTimeout(() => setMentionQuery(activeMention.query), 150);
    return () => window.clearTimeout(timeout);
  }, [activeMention, mentionMenuDismissed]);

  const mentionUsersLoadableAtom = useMemo(
    () => loadable(gitHubRepoMentionableUsersAtom(mentionQuery)),
    [mentionQuery],
  );
  const mentionUsersResult = useAtomValue(mentionUsersLoadableAtom);
  const mentionUsers = useMemo(() => {
    if (mentionUsersResult.state !== 'hasData' || activeMention == null) {
      return [];
    }
    const query = activeMention.query.toLocaleLowerCase();
    return mentionUsersResult.data
      .filter(user => user.login.toLocaleLowerCase().startsWith(query))
      .slice(0, 8);
  }, [activeMention, mentionUsersResult]);
  const mentionMenuOpen = activeMention != null && !mentionMenuDismissed;

  const updateMentionMenuPosition = useCallback(() => {
    const textarea = textareaRef.current;
    const menu = mentionMenuRef.current;
    if (textarea == null || menu == null) {
      return;
    }
    const caret = getTextareaCaretPosition(textarea);
    const textareaRect = textarea.getBoundingClientRect();
    const unclampedLeft = textareaRect.left + caret.left;
    const left = Math.max(8, Math.min(unclampedLeft, window.innerWidth - menu.offsetWidth - 8));
    const top = textareaRect.top + caret.top;
    setMentionMenuPosition(current =>
      current.left === left && current.top === top ? current : {left, top},
    );
  }, []);

  useLayoutEffect(() => {
    if (mentionMenuOpen) {
      updateMentionMenuPosition();
    }
  }, [comment, mentionMenuOpen, mentionUsers.length, mentionUsersResult.state, updateMentionMenuPosition]);

  useEffect(() => {
    if (!mentionMenuOpen) {
      return;
    }
    window.addEventListener('resize', updateMentionMenuPosition);
    window.addEventListener('scroll', updateMentionMenuPosition, true);
    return () => {
      window.removeEventListener('resize', updateMentionMenuPosition);
      window.removeEventListener('scroll', updateMentionMenuPosition, true);
    };
  }, [mentionMenuOpen, updateMentionMenuPosition]);

  useEffect(() => {
    setSelectedMentionIndex(0);
  }, [activeMention?.query, mentionUsers.length]);

  const updateActiveMention = useCallback((textarea: HTMLTextAreaElement) => {
    setActiveMention(getActiveMention(textarea.value, textarea.selectionStart));
  }, []);

  const onChange = useCallback(
    (e: ChangeEvent<HTMLTextAreaElement>) => {
      const value = e.currentTarget.value;
      setComment(value);
      updateActiveMention(e.currentTarget);
      setMentionMenuDismissed(false);
      // Clear error when user starts typing again
      if (error != null) {
        setError(null);
      }
    },
    [setComment, error, updateActiveMention],
  );

  const onSelect = useCallback(
    (e: SyntheticEvent<HTMLTextAreaElement>) => {
      const nextMention = getActiveMention(
        e.currentTarget.value,
        e.currentTarget.selectionStart,
      );
      setActiveMention(current => {
        if (
          current?.start !== nextMention?.start ||
          current?.end !== nextMention?.end ||
          current?.query !== nextMention?.query
        ) {
          setMentionMenuDismissed(false);
        }
        return nextMention;
      });
    },
    [setActiveMention],
  );

  const onAddComment = useCallback(async () => {
    setDisabled(true);
    setError(null);
    // Capture before the request. Removing focus or an inline editor while the
    // mutation is in flight can change the scroll offset before the subsequent
    // pull request refresh starts.
    setPendingScrollRestore(capturePullRequestScrollPosition());

    try {
      await addComment(comment);
    } catch (e) {
      setPendingScrollRestore(null);
      const errorMessage = formatErrorMessage(e);
      setError(errorMessage);
      // If adding the comment fails, let the user try again.
      setDisabled(false);
      return;
    }

    if (resetInputAfterAddingComment) {
      setComment(initialComment);
      setActiveMention(null);
      setMentionMenuDismissed(false);
      setDisabled(false);
    }
  }, [
    addComment,
    resetInputAfterAddingComment,
    comment,
    initialComment,
    setDisabled,
    setComment,
    setPendingScrollRestore,
  ]);

  const isAddCommentDisabled = disabled || (!allowEmptyMessage && comment.trim() === '');

  // Use a ref to avoid stale closure in onKeyDown
  const isAddCommentDisabledRef = useRef(isAddCommentDisabled);
  isAddCommentDisabledRef.current = isAddCommentDisabled;

  const insertMention = useCallback(
    (user: UserFragment) => {
      if (activeMention == null) {
        return;
      }
      const replacement = replaceActiveMention(comment, activeMention, user.login);
      setComment(replacement.text);
      setActiveMention(null);
      setMentionMenuDismissed(false);
      setError(null);
      window.requestAnimationFrame(() => {
        textareaRef.current?.focus();
        textareaRef.current?.setSelectionRange(replacement.cursor, replacement.cursor);
      });
    },
    [activeMention, comment],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      // Command+Enter (Mac) or Ctrl+Enter (Windows/Linux) to submit
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        if (!isAddCommentDisabledRef.current) {
          onAddComment();
        }
        return;
      }

      if (!mentionMenuOpen) {
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        setMentionMenuDismissed(true);
      } else if (mentionUsers.length > 0 && e.key === 'ArrowDown') {
        e.preventDefault();
        setSelectedMentionIndex(index => (index + 1) % mentionUsers.length);
      } else if (mentionUsers.length > 0 && e.key === 'ArrowUp') {
        e.preventDefault();
        setSelectedMentionIndex(index => (index - 1 + mentionUsers.length) % mentionUsers.length);
      } else if (mentionUsers.length > 0 && (e.key === 'Enter' || e.key === 'Tab')) {
        e.preventDefault();
        insertMention(mentionUsers[selectedMentionIndex]);
      }
    },
    [insertMention, mentionMenuOpen, mentionUsers, onAddComment, selectedMentionIndex],
  );

  const onInsertSuggestedChange = useCallback(() => {
    setComment(current => {
      if (current.includes('```suggestion')) {
        return current;
      }
      const replacement = current.trimEnd() || suggestedChangeText || '';
      return `\`\`\`suggestion\n${replacement}\n\`\`\``;
    });
    setActiveMention(null);
    setMentionMenuDismissed(false);
    setError(null);
  }, [suggestedChangeText]);

  const cancelButton =
    onCancel != null ? (
      <Button variant="danger" onClick={onCancel} disabled={disabled}>
        Cancel
      </Button>
    ) : null;

  const mentionMenu =
    mentionMenuOpen && typeof document !== 'undefined'
      ? createPortal(
          <div
            className="comment-mention-menu"
            id={mentionMenuID}
            ref={mentionMenuRef}
            role="listbox"
            style={mentionMenuPosition}>
            {mentionUsersResult.state === 'loading' && (
              <div className="comment-mention-status">Loading people...</div>
            )}
            {mentionUsersResult.state === 'hasError' && (
              <div className="comment-mention-status">Could not load people.</div>
            )}
            {mentionUsersResult.state === 'hasData' && mentionUsers.length === 0 && (
              <div className="comment-mention-status">No matching people.</div>
            )}
            {mentionUsers.map((user, index) => (
              <button
                className="comment-mention-option"
                id={`${mentionMenuID}-${index}`}
                key={user.id}
                type="button"
                role="option"
                aria-selected={index === selectedMentionIndex}
                onMouseEnter={() => setSelectedMentionIndex(index)}
                onMouseDown={event => {
                  event.preventDefault();
                  insertMention(user);
                }}>
                <Avatar src={user.avatarUrl} size={20} />
                <span>@{user.login}</span>
              </button>
            ))}
          </div>,
          document.body,
        )
      : null;

  return (
    <Box
      borderColor="border.default"
      borderTopWidth={1}
      borderTopStyle="solid"
      padding={1}
      width="100%">
      {error != null && (
        <Flash variant="danger" sx={{marginBottom: 2}}>
          {error}
        </Flash>
      )}
      <Box className="comment-input-editor" marginBottom={1}>
        <Textarea
          ref={textareaRef}
          value={comment}
          onChange={onChange}
          onSelect={onSelect}
          onKeyDown={onKeyDown}
          placeholder="Write a comment..."
          block={true}
          autoFocus={autoFocus}
          resize="none"
          aria-autocomplete="list"
          aria-controls={mentionMenuOpen ? mentionMenuID : undefined}
          aria-expanded={mentionMenuOpen}
          aria-activedescendant={
            mentionMenuOpen && mentionUsers.length > 0
              ? `${mentionMenuID}-${selectedMentionIndex}`
              : undefined
          }
          sx={{height: '80px'}}
        />
      </Box>
      {mentionMenu}
      <Box display="flex" justifyContent="flex-end" gridGap={1}>
        {actionSelector}
        {enableSuggestedChange && (
          <Button
            onClick={onInsertSuggestedChange}
            disabled={disabled || comment.includes('```suggestion')}>
            Suggest change
          </Button>
        )}
        {cancelButton}
        <Button disabled={isAddCommentDisabled} onClick={onAddComment} variant="primary">
          {label}
        </Button>
      </Box>
    </Box>
  );
}
