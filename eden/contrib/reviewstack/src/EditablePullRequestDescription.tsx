/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {
  commitMessageFromGhstackBody,
  replaceGhstackCommitMessage,
  stripStackInfoFromBodyHTML,
} from './ghstackUtils';
import {gitHubClientAtom, gitHubPullRequestAtom, stackedPullRequestAtom} from './jotai';
import {replaceSaplingCommitMessage, stripStackInfoFromSaplingBodyHTML} from './saplingStack';
import useRefreshPullRequest from './useRefreshPullRequest';
import {Box, Button, Flash, Textarea} from '@primer/react';
import {useAtomValue} from 'jotai';
import {useCallback, useMemo, useState} from 'react';

export default function EditablePullRequestDescription(): React.ReactElement | null {
  const client = useAtomValue(gitHubClientAtom);
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const stack = useAtomValue(stackedPullRequestAtom);
  const refreshPullRequest = useRefreshPullRequest();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const description = useMemo(() => {
    if (pullRequest == null) {
      return '';
    }
    switch (stack.type) {
      case 'sapling':
        return stack.body.commitMessage;
      case 'ghstack':
        return commitMessageFromGhstackBody(pullRequest.body);
      case 'no-stack':
        return pullRequest.body;
    }
  }, [pullRequest, stack]);

  const renderedDescription = useMemo(() => {
    if (pullRequest == null) {
      return '';
    }
    switch (stack.type) {
      case 'sapling':
        return stripStackInfoFromSaplingBodyHTML(pullRequest.bodyHTML, stack.body.format);
      case 'ghstack':
        return stripStackInfoFromBodyHTML(pullRequest.bodyHTML);
      case 'no-stack':
        return pullRequest.bodyHTML;
    }
  }, [pullRequest, stack]);

  const beginEditing = useCallback(() => {
    setDraft(description.trimEnd());
    setError(null);
    setEditing(true);
  }, [description]);

  const save = useCallback(async () => {
    if (client == null || pullRequest == null) {
      setError('Not connected to GitHub. Please refresh and try again.');
      return;
    }

    setBusy(true);
    setError(null);
    try {
      let body;
      switch (stack.type) {
        case 'sapling':
          body = replaceSaplingCommitMessage(pullRequest.body, stack.body, draft);
          break;
        case 'ghstack':
          body = replaceGhstackCommitMessage(pullRequest.body, draft);
          break;
        case 'no-stack':
          body = draft;
          break;
      }
      await client.updatePullRequest({pullRequestId: pullRequest.id, body});
      setEditing(false);
      refreshPullRequest();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [client, draft, pullRequest, refreshPullRequest, stack]);

  if (pullRequest == null) {
    return null;
  }

  if (editing) {
    return (
      <Box>
        {error != null && (
          <Flash variant="danger" sx={{marginBottom: 2}}>
            Failed to edit description: {error}
          </Flash>
        )}
        <Textarea
          aria-label="Edit pull request description"
          autoFocus={true}
          block={true}
          disabled={busy}
          resize="vertical"
          value={draft}
          onChange={event => {
            setDraft(event.currentTarget.value);
            setError(null);
          }}
          sx={{minHeight: '220px'}}
        />
        <Box display="flex" justifyContent="flex-end" gridGap={1} marginTop={2}>
          <Button disabled={busy} onClick={() => setEditing(false)}>
            Cancel
          </Button>
          <Button disabled={busy} onClick={save} variant="primary">
            Save description
          </Button>
        </Box>
      </Box>
    );
  }

  return (
    <>
      {pullRequest.viewerCanUpdate && (
        <Box display="flex" justifyContent="flex-end">
          <Button onClick={beginEditing} variant="invisible">
            Edit description
          </Button>
        </Box>
      )}
      <TrustedRenderedMarkdown trustedHTML={renderedDescription} />
    </>
  );
}
