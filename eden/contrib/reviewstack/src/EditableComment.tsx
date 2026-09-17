/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ID} from './github/types';
import type {ChangeEvent, KeyboardEvent} from 'react';

import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {gitHubClientAtom, gitHubUsernameAtom, notificationMessageAtom} from './jotai';
import useRefreshPullRequest from './useRefreshPullRequest';
import {Box, Button, Flash, Textarea} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {loadable} from 'jotai/utils';
import {useCallback, useState} from 'react';

type CommentKind = 'issue' | 'review';

type Props = {
  id: ID;
  authorLogin?: string;
  body: string;
  bodyHTML: string;
  kind: CommentKind;
  className?: string;
};

const loadableUsernameAtom = loadable(gitHubUsernameAtom);

export default function EditableComment({
  id,
  authorLogin,
  body,
  bodyHTML,
  kind,
  className,
}: Props): React.ReactElement {
  const client = useAtomValue(gitHubClientAtom);
  const usernameLoadable = useAtomValue(loadableUsernameAtom);
  const setNotification = useSetAtom(notificationMessageAtom);
  const refreshPullRequest = useRefreshPullRequest();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(body);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const username = usernameLoadable.state === 'hasData' ? usernameLoadable.data : null;
  const canModify = username != null && authorLogin === username;

  const beginEditing = useCallback(() => {
    setDraft(body);
    setError(null);
    setEditing(true);
  }, [body]);

  const cancelEditing = useCallback(() => {
    setDraft(body);
    setError(null);
    setEditing(false);
  }, [body]);

  const save = useCallback(async () => {
    if (client == null) {
      setError('Not connected to GitHub. Please refresh and try again.');
      return;
    }

    if (draft.trim() === '') {
      setError('A comment cannot be empty.');
      return;
    }

    setBusy(true);
    setError(null);
    try {
      if (kind === 'issue') {
        await client.updateIssueComment({id, body: draft});
      } else {
        await client.updatePullRequestReviewComment({
          pullRequestReviewCommentId: id,
          body: draft,
        });
      }
      setEditing(false);
      refreshPullRequest();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [client, draft, id, kind, refreshPullRequest]);

  const deleteComment = useCallback(async () => {
    if (client == null) {
      setNotification({
        type: 'error',
        message: 'Not connected to GitHub. Please refresh and try again.',
      });
      return;
    }
    if (!window.confirm('Delete this comment permanently?')) {
      return;
    }

    setBusy(true);
    try {
      if (kind === 'issue') {
        await client.deleteIssueComment({id});
      } else {
        await client.deletePullRequestReviewComment({id});
      }
      refreshPullRequest();
      setBusy(false);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setNotification({type: 'error', message: `Failed to delete comment: ${message}`});
      setBusy(false);
    }
  }, [client, id, kind, refreshPullRequest, setNotification]);

  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        if (!busy) {
          save();
        }
      }
      if (event.key === 'Escape' && !busy) {
        cancelEditing();
      }
    },
    [busy, cancelEditing, save],
  );

  if (editing) {
    return (
      <Box marginTop={1}>
        {error != null && (
          <Flash variant="danger" sx={{marginBottom: 2}}>
            Failed to edit comment: {error}
          </Flash>
        )}
        <Textarea
          aria-label="Edit comment"
          autoFocus={true}
          block={true}
          disabled={busy}
          resize="vertical"
          value={draft}
          onChange={(event: ChangeEvent<HTMLTextAreaElement>) => {
            setDraft(event.currentTarget.value);
            setError(null);
          }}
          onKeyDown={onKeyDown}
          sx={{minHeight: '100px'}}
        />
        <Box display="flex" justifyContent="flex-end" gridGap={1} marginTop={1}>
          <Button disabled={busy} onClick={cancelEditing}>
            Cancel
          </Button>
          <Button disabled={busy || draft.trim() === ''} onClick={save} variant="primary">
            Save
          </Button>
        </Box>
      </Box>
    );
  }

  return (
    <>
      {canModify && (
        <Box display="flex" justifyContent="flex-end" gridGap={1}>
          <Button disabled={busy} onClick={beginEditing} variant="invisible">
            Edit
          </Button>
          <Button
            disabled={busy}
            onClick={deleteComment}
            variant="invisible"
            sx={{color: 'danger.fg'}}>
            Delete
          </Button>
        </Box>
      )}
      <TrustedRenderedMarkdown className={className} trustedHTML={bodyHTML} />
    </>
  );
}
