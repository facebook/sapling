/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  AddCommentMutationData,
  AddLabelsToLabelableInput,
  AddLabelsToLabelableMutationData,
  AddPullRequestReviewInput,
  AddPullRequestReviewMutationData,
  AddPullRequestReviewCommentInput,
  AddPullRequestReviewCommentMutationData,
  LabelFragment,
  RemoveLabelsFromLabelableInput,
  RemoveLabelsFromLabelableMutationData,
  RequestReviewsInput,
  RequestReviewsMutationData,
  StackPullRequestFragment,
  SubmitPullRequestReviewInput,
  SubmitPullRequestReviewMutationData,
  UserFragment,
} from '../generated/graphql';
import type {PullRequest} from './pullRequestTimelineTypes';
import type {PullsQueryInput, PullsWithPageInfo} from './pullsTypes';
import type {CommitComparison} from './restApiTypes';
import type {Blob, Commit, GitObjectID, ID, Tree} from './types';

/**
 * Client for fetching data from GitHub. Intended to abstract away
 * the caching/fetching strategy used under the hood. Note that GitHub APIs
 * have various quotas. For example, their GraphQL API v4 rate limit is
 * 5000 "points" per hour:
 *
 * https://docs.github.com/en/graphql/overview/resource-limitations
 *
 * As such, using local caching or perhaps even reading from a local clone via
 * the HTML5 FileSystem API could be used to help stay within GitHub quota.
 */
export default interface GitHubClient {
  getCommit(oid: GitObjectID): Promise<Commit | null>;
  getCommitComparison(base: GitObjectID, head: GitObjectID): Promise<CommitComparison | null>;
  getTree(oid: GitObjectID): Promise<Tree | null>;
  getBlob(oid: GitObjectID): Promise<Blob | null>;
  getPullRequest(pr: number): Promise<PullRequest | null>;
  getPullRequests(input: PullsQueryInput): Promise<PullsWithPageInfo | null>;
  getRepoAssignableUsers(query: string | null): Promise<UserFragment[]>;
  getRepoLabels(query: string | null): Promise<LabelFragment[]>;
  getStackPullRequests(prs: number[]): Promise<StackPullRequestFragment[]>;

  /**
   * Add a comment to an issue or pull request:
   * https://docs.github.com/en/graphql/reference/mutations#addcomment
   *
   * Note this requires an ID, which can found on the result of
   * getPullRequest().
   */
  addComment(id: ID, body: string): Promise<AddCommentMutationData>;

  /**
   * Adds labels to a labelable object.
   * https://docs.github.com/en/graphql/reference/mutations#addlabelstolabelable
   */
  addLabels(input: AddLabelsToLabelableInput): Promise<AddLabelsToLabelableMutationData>;

  /**
   * Adds a review to a Pull Request.
   * https://docs.github.com/en/graphql/reference/mutations#addpullrequestreview
   */
  addPullRequestReview(input: AddPullRequestReviewInput): Promise<AddPullRequestReviewMutationData>;

  /**
   * Adds a comment to a pull request review.
   * https://docs.github.com/en/graphql/reference/mutations#addpullrequestreviewcomment
   */
  addPullRequestReviewComment(
    input: AddPullRequestReviewCommentInput,
  ): Promise<AddPullRequestReviewCommentMutationData>;

  /**
   * Removes labels from a Labelable object.
   * https://docs.github.com/en/graphql/reference/mutations#removelabelsfromlabelable
   */
  removeLabels(
    input: RemoveLabelsFromLabelableInput,
  ): Promise<RemoveLabelsFromLabelableMutationData>;

  /**
   * Set review requests on a pull request..
   * https://docs.github.com/en/graphql/reference/mutations#requestreviews
   */
  requestReviews(input: RequestReviewsInput): Promise<RequestReviewsMutationData>;

  /**
   * Submits a pending pull request review.
   * https://docs.github.com/en/graphql/reference/mutations#submitpullrequestreview
   */
  submitPullRequestReview(
    input: SubmitPullRequestReviewInput,
  ): Promise<SubmitPullRequestReviewMutationData>;
}
