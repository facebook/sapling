/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  ActorFragment,
  CommitFragment,
  DiffSide,
  PullRequestQueryData,
  PullRequestReviewCommentState,
  PullRequestReviewCommentFragment,
  PullRequestReviewThreadFragment,
  PullRequestTimelineItemFragment,
  PullRequestTimelineItemFragment_ClosedEvent_,
  PullRequestTimelineItemFragment_HeadRefForcePushedEvent_,
  PullRequestTimelineItemFragment_IssueComment_,
  PullRequestTimelineItemFragment_MergedEvent_,
  PullRequestTimelineItemFragment_PullRequestCommit_,
  PullRequestTimelineItemFragment_PullRequestReview_,
  PullRequestTimelineItemFragment_RenamedTitleEvent_,
  PullRequestTimelineItemFragment_ReviewRequestRemovedEvent_,
  PullRequestTimelineItemFragment_ReviewRequestedEvent_,
} from '../generated/graphql';
import type {GitObject, ID} from './types';

export type Actor = ActorFragment;

export type PullRequestReviewState =
  | 'PENDING'
  | 'COMMENTED'
  | 'APPROVED'
  | 'CHANGES_REQUESTED'
  | 'DISMISSED';

export type CommitData = CommitFragment;

type Repository = NonNullable<PullRequestQueryData['repository']>;

export type PullRequest = NonNullable<Repository['pullRequest']>;

export type PullRequestReviewThread = PullRequestReviewThreadFragment;

export type GitHubPullRequestReviewThread = {
  /**
   * In the timeline, we expect there to be a PullRequestReview object
   * (__typename is PULL_REQUEST_REVIEW) whose `comments.nodes[0].id` matches
   * this ID.
   */
  firstCommentID: ID;
  originalLine: number | null | undefined;
  diffSide: DiffSide;
  comments: GitHubPullRequestReviewThreadComment[];
};

export type GitHubPullRequestReviewThreadComment = {
  id: string;
  author: Actor | null;
  originalCommit?: GitObject | null;
  path: string;
  bodyHTML: string;
  state: PullRequestReviewCommentState;
};

export type GitHubPullRequestReviewThreadsByLine = Map<number, GitHubPullRequestReviewThread[]>;

export type PullRequestReviewComment = {
  originalLine: PullRequestReviewThread['originalLine'];
  comment: PullRequestComment;
};

export type PullRequestTimelineItem = PullRequestTimelineItemFragment;

export type PullRequestComment = PullRequestReviewCommentFragment;

export type PullRequestCommitItem = PullRequestTimelineItemFragment_PullRequestCommit_;

export type PullRequestReviewItem = PullRequestTimelineItemFragment_PullRequestReview_;

export type HeadRefForcePushedEventItem = PullRequestTimelineItemFragment_HeadRefForcePushedEvent_;

export type ReviewRequestedEventItem = PullRequestTimelineItemFragment_ReviewRequestedEvent_;

export type ReviewRequestRemovedEventItem =
  PullRequestTimelineItemFragment_ReviewRequestRemovedEvent_;

export type IssueCommentItem = PullRequestTimelineItemFragment_IssueComment_;

export type RenamedTitleEventItem = PullRequestTimelineItemFragment_RenamedTitleEvent_;

export type MergedEventItem = PullRequestTimelineItemFragment_MergedEvent_;

export type ClosedEventItem = PullRequestTimelineItemFragment_ClosedEvent_;
