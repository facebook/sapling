/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './PullRequestTimeline.css';

import type {
  Actor,
  ClosedEventItem,
  HeadRefForcePushedEventItem,
  IssueCommentItem,
  MergedEventItem,
  PullRequestReviewItem,
  PullRequestTimelineItem,
  RenamedTitleEventItem,
  ReviewRequestedEventItem,
  ReviewRequestRemovedEventItem,
} from './github/pullRequestTimelineTypes';

import ActorHeading from './ActorHeading';
import CenteredSpinner from './CenteredSpinner';
import CommitLink from './CommitLink';
import PendingLabel from './PendingLabel';
import PullRequestReviewComment from './PullRequestReviewComment';
import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {
  gitHubOrgAndRepo,
  gitHubPullRequest,
  gitHubPullRequestReviewThreadsByFirstCommentID,
} from './recoil';
import {
  CheckCircleFillIcon,
  FileDiffIcon,
  GitMergeIcon,
  GitPullRequestClosedIcon,
} from '@primer/octicons-react';
import {Box, StyledOcticon, Text} from '@primer/react';
import React, {Suspense} from 'react';
import {useRecoilValue} from 'recoil';
import {notEmpty} from 'shared/utils';

export default function PullRequestTimeline(): React.ReactElement {
  return (
    <Suspense fallback={<CenteredSpinner />}>
      <PullRequestTimelineBootstrap />
    </Suspense>
  );
}

function PullRequestTimelineBootstrap(): React.ReactElement {
  const pullRequest = useRecoilValue(gitHubPullRequest);
  const items = (pullRequest?.timelineItems.nodes ?? []).filter(notEmpty);
  let version = 1;
  return (
    <Box display="flex" flexDirection="column" gridGap={2}>
      <VersionBreak version={1} />
      {items.map((item, index) => {
        if (item.__typename === 'HeadRefForcePushedEvent') {
          ++version;
        }
        return <TimelineItem key={index} item={item} version={version} />;
      })}

      {/* Adds some padding after the last TimelineItem. */}
      <TimelineBasicEvent />
    </Box>
  );
}

function TimelineItem({
  item,
  version,
}: {
  item: PullRequestTimelineItem;
  version: number;
}): React.ReactElement | null {
  switch (item.__typename) {
    case 'PullRequestCommit':
      return null;
    case 'PullRequestReview':
      return <PullRequestReview item={item} />;
    case 'IssueComment':
      return <IssueComment item={item} />;
    case 'HeadRefForcePushedEvent':
      return <HeadRefForcePushedEvent item={item} version={version} />;
    case 'RenamedTitleEvent':
      return <RenamedTitleEvent item={item} />;
    case 'ReviewRequestedEvent':
      return <ReviewRequestEvent item={item} isRequested={true} />;
    case 'ReviewRequestRemovedEvent':
      return <ReviewRequestEvent item={item} isRequested={false} />;
    case 'MergedEvent':
      return <MergedEvent item={item} />;
    case 'ClosedEvent':
      return <ClosedEvent item={item} />;
    default:
      return null;
  }
}

function TimelineCallout(props: {
  actor?: Actor | null;
  children: React.ReactNode;
  isPending?: boolean;
}): React.ReactElement {
  return (
    <Box padding="4px 6px 0">
      <Box
        backgroundColor="canvas.default"
        color="fg.default"
        border={1}
        borderColor="border.default"
        borderStyle="solid"
        borderRadius="2px"
        padding="6px 8px">
        <Box display="flex" justifyContent="space-between">
          <Box display="flex" gridGap={1}>
            <ActorHeading actor={props.actor} /> <Text fontSize={12}>commented</Text>
          </Box>
          {props.isPending && <PendingLabel />}
        </Box>
        {props.children}
      </Box>
    </Box>
  );
}

function TimelineBasicEvent(props: {children?: React.ReactNode}): React.ReactElement {
  return (
    <Box color="neutral.emphasis" fontSize="12px" lineHeight="14px" padding="4px 6px 0">
      {props.children}
    </Box>
  );
}

function PullRequestReview({item}: {item: PullRequestReviewItem}): React.ReactElement {
  const threadMap = useRecoilValue(gitHubPullRequestReviewThreadsByFirstCommentID);
  const comments = (item.comments.nodes ?? []).filter(notEmpty).flatMap((comment, index) => {
    // Check and see whether the first comment in the PullRequestReview corresponds
    // to a thread. If so, the comments in the thread are not available as
    // timeline items, but must be pulled from the `reviewThreads` field on the
    // PullRequest.
    const thread = threadMap[comment.id];
    if (thread) {
      // TODO: Honor the replyTo field on PullRequestReviewComment to show the
      // appropriate level of depth within the thread.
      return thread.comments.map((comment, threadIndex) => (
        <PullRequestReviewComment key={`${index}/${threadIndex}`} comment={comment} />
      ));
    } else {
      return [<PullRequestReviewComment key={index} comment={comment} />];
    }
  });
  let action = null;
  let isPending = false;
  switch (item.state) {
    // TODO(mbolin): Handle other cases? Need to find examples on GitHub.
    case 'APPROVED': {
      action = (
        <ReviewAction
          actor={item.author}
          action="approved these changes"
          color="success.fg"
          icon={CheckCircleFillIcon}
        />
      );
      break;
    }
    case 'CHANGES_REQUESTED': {
      action = (
        <ReviewAction
          actor={item.author}
          action="requested changes"
          color="danger.fg"
          icon={FileDiffIcon}
        />
      );
      break;
    }
    case 'PENDING': {
      isPending = true;
      break;
    }
  }
  const hasContent = item.bodyHTML !== '' || comments.length > 0;

  return (
    <>
      {action}
      {hasContent && (
        <TimelineCallout actor={item.author} isPending={isPending}>
          {item.bodyHTML !== '' && (
            <Box paddingY={1}>
              <TrustedRenderedMarkdown
                className="PRT-bodyHTML PRT-review-comment-text"
                trustedHTML={item.bodyHTML}
              />
            </Box>
          )}
          {comments.length > 0 && (
            <>
              <Box display="flex" alignItems="center" paddingY={1}>
                <Text fontSize="0.5rem" fontWeight="bold">
                  INLINE COMMENTS
                </Text>
                <HorizontalLine isPrefix={false} />
              </Box>

              {comments}
            </>
          )}
        </TimelineCallout>
      )}
    </>
  );
}

function ReviewAction({
  actor,
  action,
  color,
  icon,
}: {
  actor?: Actor | null;
  action: string;
  color: string;
  icon: React.ElementType;
}) {
  return (
    <TimelineBasicEvent>
      <Box display="flex" gridGap={1} alignItems="center">
        <StyledOcticon icon={icon} color={color} /> <ActorBasic actor={actor} /> {action}
      </Box>
    </TimelineBasicEvent>
  );
}

function IssueComment({item}: {item: IssueCommentItem}): React.ReactElement {
  return (
    <TimelineCallout actor={item.author}>
      <TrustedRenderedMarkdown
        className="PRT-bodyHTML PRT-review-comment-text"
        trustedHTML={item.bodyHTML}
      />
    </TimelineCallout>
  );
}

function HeadRefForcePushedEvent({
  item,
  version,
}: {
  item: HeadRefForcePushedEventItem;
  version: number;
}): React.ReactElement {
  return (
    <>
      <VersionBreak version={version} />
      <TimelineBasicEvent>
        <ActorBasic actor={item.actor} /> updated this PR to V{version} (
        <code>{item.afterCommit?.oid.slice(0, 8)}</code>)
      </TimelineBasicEvent>
    </>
  );
}

function HorizontalLine({isPrefix}: {isPrefix: boolean}): React.ReactElement {
  return (
    <Box
      borderBottomStyle="solid"
      borderBottomWidth={1}
      borderColor="border.subtle"
      display="block"
      fontSize="0.625rem"
      fontWeight="bold"
      lineHeight={1.2}
      flexBasis={isPrefix ? '24px' : 'autho'}
      flexGrow={isPrefix ? 0 : 1}
      flexShrink={isPrefix ? 0 : 1}
      marginLeft={isPrefix ? 0 : '8px'}
      marginRight={isPrefix ? '8px' : 0}>
      {' '}
    </Box>
  );
}

function VersionBreak({version}: {version: number}): React.ReactElement {
  return (
    <Box paddingTop={1} paddingX="6px">
      <Box
        display="flex"
        alignItems="center"
        fontSize="10px"
        fontWeight={700}
        lineHeight="12px"
        paddingX={0}
        paddingY={1}>
        <HorizontalLine isPrefix={true} />
        <Text color="neutral.emphasis">V{version}</Text>
        <HorizontalLine isPrefix={false} />
      </Box>
    </Box>
  );
}

function ReviewRequestEvent({
  item,
  isRequested,
}: {
  item: ReviewRequestedEventItem | ReviewRequestRemovedEventItem;
  isRequested: boolean;
}): React.ReactElement {
  const requestedReviewer =
    item.requestedReviewer?.__typename === 'User' ||
    item.requestedReviewer?.__typename === 'Mannequin'
      ? item.requestedReviewer
      : null;
  const action = isRequested ? 'requested a review from' : 'removed review request for';

  return (
    <TimelineBasicEvent>
      <ActorBasic actor={item.actor} /> {action} <ActorBasic actor={requestedReviewer} />
    </TimelineBasicEvent>
  );
}

function RenamedTitleEvent({item}: {item: RenamedTitleEventItem}): React.ReactElement {
  return (
    <TimelineBasicEvent>
      <ActorBasic actor={item.actor} /> changed the title{' '}
      <span className="PRT-old-title">{item.previousTitle}</span> to{' '}
      <span className="PRT-new-title">{item.currentTitle}</span>
    </TimelineBasicEvent>
  );
}

function MergedEvent({item}: {item: MergedEventItem}) {
  const {org, repo} = useRecoilValue(gitHubOrgAndRepo) ?? {};
  const {actor, mergedCommit, mergeRefName} = item;
  const commitOID = mergedCommit?.oid;
  const commit =
    commitOID == null ? null : org == null || repo == null ? (
      commitOID
    ) : (
      <CommitLink org={org} repo={repo} oid={commitOID} />
    );

  return (
    <TimelineBasicEvent>
      <Box display="flex" gridGap={1} alignItems="center">
        <>
          <StyledOcticon icon={GitMergeIcon} color="done.fg" /> <ActorBasic actor={actor} /> merged
          commit {commit} into {mergeRefName}
        </>
      </Box>
    </TimelineBasicEvent>
  );
}

function ClosedEvent({item}: {item: ClosedEventItem}) {
  if (item.closable.__typename === 'PullRequest' && item.closable.merged) {
    return null;
  }

  return (
    <TimelineBasicEvent>
      <Box display="flex" gridGap={1} alignItems="center">
        <>
          <StyledOcticon icon={GitPullRequestClosedIcon} color="danger.fg" />{' '}
          <ActorBasic actor={item.actor} /> closed this pull request
        </>
      </Box>
    </TimelineBasicEvent>
  );
}

/**
 * Note that `ReviewRequestedEventItem.requestedReviewer` is not an `Actor`,
 * hence the duck type for the `actor` prop.
 */
function ActorBasic({actor}: {actor?: {login: string} | null}): React.ReactElement {
  const login = actor?.login ?? '[unknown]';
  return <span className="PRT-actor-basic">{login}</span>;
}
