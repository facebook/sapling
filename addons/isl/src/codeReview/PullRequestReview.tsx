/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {DiffLineLocation} from '../ComparisonView/SplitDiffView/types';
import type {
  DiffId,
  Hash,
  PullRequestReviewAction,
  PullRequestReviewComment,
  PullRequestReviewData,
  PullRequestReviewEvent,
  PullRequestReviewThread,
} from '../types';

import {Button} from 'isl-components/Button';
import {ErrorNotice} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {TextArea} from 'isl-components/TextArea';
import {useAtom} from 'jotai';
import {useCallback, useEffect, useMemo, useRef, useState} from 'react';
import {Link} from '../Link';
import platform from '../platform';
import {pullRequestReviewData, runPullRequestReviewAction} from './pullRequestReviewState';

import css from './PullRequestReview.module.css';

type ReviewTarget = {diffId: DiffId; commitOid: Hash; pullRequestHead: Hash};

export type PullRequestReviewController = {
  header: ReactNode;
  onStartComment?: (location: DiffLineLocation, suggestedText?: string) => void;
  isLineCommented?: (location: DiffLineLocation) => boolean;
  renderLineAddon?: (location: DiffLineLocation) => ReactNode;
};

export function usePullRequestReviewController(
  target: ReviewTarget | undefined,
): PullRequestReviewController {
  const diffId = target?.diffId ?? '';
  const [loadedReview, refreshReview] = useAtom(pullRequestReviewData(diffId));
  const [review, setReview] = useState<PullRequestReviewData>();
  const [activeLocation, setActiveLocation] = useState<DiffLineLocation>();
  const [suggestedText, setSuggestedText] = useState<string>();
  const [draft, setDraft] = useState('');
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  const lastTargetKey = useRef<string | undefined>(undefined);

  useEffect(() => {
    if (loadedReview.state === 'hasData') {
      setReview(loadedReview.data);
    }
  }, [loadedReview]);

  useEffect(() => {
    const targetKey =
      target == null
        ? undefined
        : `${target.diffId}\0${target.commitOid}\0${target.pullRequestHead}`;
    if (lastTargetKey.current === targetKey) {
      return;
    }
    lastTargetKey.current = targetKey;
    setReview(undefined);
    setActiveLocation(undefined);
    setSuggestedText(undefined);
    setDraft('');
    setError(undefined);
    // The atom keeps its value when the comparison closes. Refresh cached data
    // when this review is opened again, but do not duplicate an initial request.
    if (target != null && loadedReview.state !== 'loading') {
      refreshReview();
    }
  }, [loadedReview.state, refreshReview, target]);

  useEffect(() => {
    if (target == null) {
      return;
    }
    const refreshOnFocus = () => refreshReview();
    window.addEventListener('focus', refreshOnFocus);
    return () => window.removeEventListener('focus', refreshOnFocus);
  }, [refreshReview, target]);

  const perform = useCallback(
    async (action: PullRequestReviewAction): Promise<boolean> => {
      if (target == null) {
        return false;
      }
      setBusy(true);
      setError(undefined);
      try {
        setReview(await runPullRequestReviewAction(target.diffId, action));
        return true;
      } catch (caught) {
        setError(caught instanceof Error ? caught : new Error(String(caught)));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [target],
  );

  const stale = review != null && target != null && review.headOid !== target.pullRequestHead;
  const threadsByLine = useMemo(() => {
    const result = new Map<string, PullRequestReviewThread[]>();
    if (stale) {
      return result;
    }
    for (const thread of review?.threads ?? []) {
      const line = thread.line ?? thread.originalLine;
      if (line == null) {
        continue;
      }
      const key = lineKey({path: thread.path, line, side: thread.side});
      const existing = result.get(key);
      if (existing == null) {
        result.set(key, [thread]);
      } else {
        existing.push(thread);
      }
    }
    return result;
  }, [review?.threads, stale]);
  const commentedLines = useMemo(() => {
    const result = new Set<string>();
    if (stale) {
      return result;
    }
    for (const thread of review?.threads ?? []) {
      const endLine = thread.line ?? thread.originalLine;
      if (endLine == null) {
        continue;
      }
      const startLine = thread.startLine ?? thread.originalStartLine ?? endLine;
      for (let line = Math.min(startLine, endLine); line <= Math.max(startLine, endLine); line++) {
        result.add(lineKey({path: thread.path, line, side: thread.side}));
      }
    }
    return result;
  }, [review?.threads, stale]);
  if (target == null) {
    return {header: null};
  }

  const loading = loadedReview.state === 'loading' && review == null;
  const loadError = loadedReview.state === 'hasError' ? asError(loadedReview.error) : undefined;
  const visibleError = error ?? loadError;
  const canComment = review != null && !stale;

  const onStartComment = canComment
    ? (location: DiffLineLocation, text?: string) => {
        setActiveLocation(location);
        setSuggestedText(text);
        setDraft('');
        setError(undefined);
      }
    : undefined;

  const renderLineAddon = canComment
    ? (location: DiffLineLocation) => {
        const threads = threadsByLine.get(lineKey(location)) ?? [];
        const isActive = activeLocation != null && lineKey(activeLocation) === lineKey(location);
        const reviewLocation = isActive && activeLocation != null ? activeLocation : location;
        if (threads.length === 0 && !isActive) {
          return null;
        }
        return (
          <ReviewLine
            location={reviewLocation}
            threads={threads}
            active={isActive}
            draft={draft}
            setDraft={setDraft}
            suggestedText={suggestedText}
            pendingReview={review?.pendingReviewId != null}
            busy={busy}
            error={isActive ? visibleError : undefined}
            onCancel={() => {
              setActiveLocation(undefined);
              setSuggestedText(undefined);
              setDraft('');
              setError(undefined);
            }}
            onCreate={async mode => {
              const succeeded = await perform({
                type: 'createComment',
                body: draft,
                path: reviewLocation.path,
                line: reviewLocation.line,
                side: reviewLocation.side,
                startLine: reviewLocation.startLine,
                startSide: reviewLocation.startSide,
                mode,
                commitOid: target.commitOid,
                expectedHeadOid: target.pullRequestHead,
              });
              if (succeeded) {
                setActiveLocation(undefined);
                setSuggestedText(undefined);
                setDraft('');
              }
            }}
            perform={perform}
          />
        );
      }
    : undefined;

  return {
    header: (
      <ReviewToolbar
        diffId={target.diffId}
        pullRequestHead={target.pullRequestHead}
        review={review}
        loading={loading}
        stale={stale}
        busy={busy}
        error={visibleError}
        refresh={() => {
          setError(undefined);
          refreshReview();
        }}
        perform={perform}
      />
    ),
    onStartComment,
    isLineCommented: location => commentedLines.has(lineKey(location)),
    renderLineAddon,
  };
}

function ReviewToolbar({
  diffId,
  pullRequestHead,
  review,
  loading,
  stale,
  busy,
  error,
  refresh,
  perform,
}: {
  diffId: DiffId;
  pullRequestHead: Hash;
  review?: PullRequestReviewData;
  loading: boolean;
  stale: boolean;
  busy: boolean;
  error?: Error;
  refresh: () => void;
  perform: (action: PullRequestReviewAction) => Promise<boolean>;
}) {
  const [summary, setSummary] = useState('');
  const pendingCount =
    review?.threads.filter(thread => thread.comments.some(comment => comment.state === 'PENDING'))
      .length ?? 0;

  const submit = async (event: PullRequestReviewEvent) => {
    if (
      await perform({
        type: 'submitReview',
        event,
        body: summary,
        expectedHeadOid: pullRequestHead,
      })
    ) {
      setSummary('');
    }
  };

  return (
    <div className={css.toolbar} data-testid="pull-request-review-toolbar">
      <div className={css.toolbarTitle}>
        <Icon icon="comment-discussion" />
        <b>Review PR #{diffId}</b>
        {loading && <Icon icon="loading" />}
        {review?.pendingReviewId != null && (
          <span className={css.pendingBadge}>{pendingCount} pending</span>
        )}
        <Button icon aria-label="Refresh review comments" onClick={refresh} disabled={busy}>
          <Icon icon="refresh" />
        </Button>
      </div>
      {stale && (
        <div className={css.warning} role="alert">
          The local commit does not match the current pull request head. Download the latest PR
          version before commenting.
        </div>
      )}
      {error != null && <ErrorNotice title="GitHub review action failed" error={error} />}
      {!stale && review != null && (
        <details className={css.submitReview}>
          <summary>
            {review.pendingReviewId == null ? 'Submit review' : 'Submit pending review'}
          </summary>
          <TextArea
            value={summary}
            onChange={event => setSummary(event.currentTarget.value)}
            placeholder="Review summary (optional)"
            rows={3}
            resize="vertical"
          />
          <div className={css.actions}>
            <Button disabled={busy} onClick={() => submit('COMMENT')}>
              Comment
            </Button>
            <Button disabled={busy} onClick={() => submit('APPROVE')}>
              Approve
            </Button>
            <Button disabled={busy} onClick={() => submit('REQUEST_CHANGES')}>
              Request changes
            </Button>
          </div>
        </details>
      )}
    </div>
  );
}

function ReviewLine({
  location,
  threads,
  active,
  draft,
  setDraft,
  suggestedText,
  pendingReview,
  busy,
  error,
  onCancel,
  onCreate,
  perform,
}: {
  location: DiffLineLocation;
  threads: PullRequestReviewThread[];
  active: boolean;
  draft: string;
  setDraft: (value: string) => void;
  suggestedText?: string;
  pendingReview: boolean;
  busy: boolean;
  error?: Error;
  onCancel: () => void;
  onCreate: (mode: 'single' | 'pending') => Promise<void>;
  perform: (action: PullRequestReviewAction) => Promise<boolean>;
}) {
  return (
    <div
      className={css.lineReview}
      data-review-path={location.path}
      data-review-line={location.line}>
      {threads.map(thread => (
        <ReviewThread key={thread.id} thread={thread} busy={busy} perform={perform} />
      ))}
      {active && (
        <div className={css.composer}>
          <TextArea
            autoFocus
            value={draft}
            onChange={event => setDraft(event.currentTarget.value)}
            placeholder={`Comment on ${formatLocation(location)}`}
            rows={4}
            resize="vertical"
          />
          {error != null && <div className={css.actionError}>{error.message}</div>}
          <div className={css.actions}>
            {suggestedText != null && (
              <Button
                onClick={() => setDraft(appendSuggestion(draft, suggestedText))}
                disabled={busy}>
                Suggest change
              </Button>
            )}
            <Button onClick={onCancel} disabled={busy}>
              Cancel
            </Button>
            <Button onClick={() => onCreate('single')} disabled={busy || draft.trim() === ''}>
              Add single comment
            </Button>
            <Button
              primary
              onClick={() => onCreate('pending')}
              disabled={busy || draft.trim() === ''}>
              {pendingReview ? 'Add to review' : 'Start review'}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function appendSuggestion(draft: string, suggestedText: string): string {
  const suggestion = `\`\`\`suggestion\n${suggestedText}\n\`\`\``;
  const prefix = draft.trimEnd();
  return prefix === '' ? suggestion : `${prefix}\n\n${suggestion}`;
}

function formatLocation(location: DiffLineLocation): string {
  const lines =
    location.startLine == null ? `${location.line}` : `${location.startLine}-${location.line}`;
  return `${location.path}:${lines}`;
}

function ReviewThread({
  thread,
  busy,
  perform,
}: {
  thread: PullRequestReviewThread;
  busy: boolean;
  perform: (action: PullRequestReviewAction) => Promise<boolean>;
}) {
  const [collapsed, setCollapsed] = useState(thread.isResolved);
  const [replying, setReplying] = useState(false);
  const [reply, setReply] = useState('');
  const [editing, setEditing] = useState<PullRequestReviewComment>();
  const [editBody, setEditBody] = useState('');
  const rootComment = thread.comments[0];

  useEffect(() => {
    setCollapsed(thread.isResolved);
  }, [thread.isResolved]);

  return (
    <div className={`${css.thread} ${thread.isResolved ? css.resolved : ''}`}>
      {thread.isResolved && (
        <div className={css.resolvedHeader}>
          <Button
            icon
            aria-label={collapsed ? 'Show resolved comment' : 'Hide resolved comment'}
            aria-expanded={!collapsed}
            onClick={() => setCollapsed(value => !value)}>
            <Icon icon={collapsed ? 'chevron-down' : 'chevron-up'} />
          </Button>
          <span className={css.resolvedSummary}>
            Resolved{rootComment == null ? '' : ` · ${rootComment.author}`}
            {thread.comments.length > 1 ? ` · ${thread.comments.length} comments` : ''}
          </span>
        </div>
      )}
      {!collapsed && (
        <>
          {thread.isOutdated && <div className={css.outdated}>Outdated</div>}
          {thread.comments.map(comment => (
            <div className={css.comment} key={comment.id}>
              <img className={css.avatar} src={comment.authorAvatarUri} alt="" />
              <div className={css.commentBody}>
                <div className={css.commentHeader}>
                  <b>{comment.author}</b>
                  {comment.state === 'PENDING' && <span className={css.pendingBadge}>Pending</span>}
                  <Link href={comment.url}>Open on GitHub</Link>
                </div>
                {editing?.id === comment.id ? (
                  <>
                    <TextArea
                      value={editBody}
                      onChange={event => setEditBody(event.currentTarget.value)}
                      rows={3}
                      resize="vertical"
                    />
                    <div className={css.actions}>
                      <Button onClick={() => setEditing(undefined)} disabled={busy}>
                        Cancel
                      </Button>
                      <Button
                        primary
                        disabled={busy || editBody.trim() === ''}
                        onClick={async () => {
                          if (
                            comment.databaseId != null &&
                            (await perform({
                              type: 'editComment',
                              commentDatabaseId: comment.databaseId,
                              body: editBody,
                            }))
                          ) {
                            setEditing(undefined);
                          }
                        }}>
                        Save
                      </Button>
                    </div>
                  </>
                ) : (
                  <div
                    className="rendered-markup"
                    dangerouslySetInnerHTML={{__html: comment.html}}
                  />
                )}
                {editing?.id !== comment.id &&
                  (comment.viewerCanUpdate || comment.viewerCanDelete) && (
                    <div className={css.commentActions}>
                      {comment.viewerCanUpdate && comment.databaseId != null && (
                        <Button
                          icon
                          aria-label="Edit comment"
                          disabled={busy}
                          onClick={() => {
                            setEditing(comment);
                            setEditBody(comment.body);
                          }}>
                          <Icon icon="edit" />
                        </Button>
                      )}
                      {comment.viewerCanDelete && comment.databaseId != null && (
                        <Button
                          icon
                          aria-label="Delete comment"
                          disabled={busy}
                          onClick={async () => {
                            if (
                              comment.databaseId != null &&
                              (await platform.confirm('Delete this review comment?')) === true
                            ) {
                              await perform({
                                type: 'deleteComment',
                                commentDatabaseId: comment.databaseId,
                              });
                            }
                          }}>
                          <Icon icon="trash" />
                        </Button>
                      )}
                    </div>
                  )}
              </div>
            </div>
          ))}
          <div className={css.threadActions}>
            {thread.viewerCanReply && rootComment?.databaseId != null && (
              <Button onClick={() => setReplying(value => !value)} disabled={busy}>
                Reply
              </Button>
            )}
            {thread.isResolved
              ? thread.viewerCanUnresolve && (
                  <Button
                    onClick={() =>
                      perform({type: 'setResolved', threadId: thread.id, resolved: false})
                    }
                    disabled={busy}>
                    Reopen
                  </Button>
                )
              : thread.viewerCanResolve && (
                  <Button
                    onClick={() =>
                      perform({type: 'setResolved', threadId: thread.id, resolved: true})
                    }
                    disabled={busy}>
                    Resolve
                  </Button>
                )}
          </div>
          {replying && rootComment?.databaseId != null && (
            <div className={css.replyComposer}>
              <TextArea
                autoFocus
                value={reply}
                onChange={event => setReply(event.currentTarget.value)}
                placeholder="Reply to this thread"
                rows={3}
                resize="vertical"
              />
              <div className={css.actions}>
                <Button onClick={() => setReplying(false)} disabled={busy}>
                  Cancel
                </Button>
                <Button
                  primary
                  disabled={busy || reply.trim() === ''}
                  onClick={async () => {
                    if (
                      rootComment.databaseId != null &&
                      (await perform({
                        type: 'reply',
                        commentDatabaseId: rootComment.databaseId,
                        body: reply,
                      }))
                    ) {
                      setReply('');
                      setReplying(false);
                    }
                  }}>
                  Reply
                </Button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function lineKey(location: {path: string; line: number; side: string}): string {
  return `${location.path}\0${location.side}\0${location.line}`;
}

function asError(value: unknown): Error {
  return value instanceof Error ? value : new Error(String(value));
}
