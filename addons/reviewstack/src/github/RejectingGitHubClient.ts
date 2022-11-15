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
import type GitHubClient from './GitHubClient';
import type {PullRequest} from './pullRequestTimelineTypes';
import type {PullsQueryInput, PullsWithPageInfo} from './pullsTypes';
import type {CommitComparison} from './restApiTypes';
import type {Blob, Commit, GitObjectID, ID, Tree} from './types';

/**
 * GitHubClient that fails for all methods. Designed to be used with other
 * implementations of GitHubClient that use the decorator pattern, but never
 * intend to rely on the fallback method.
 */
export default class RejectingGitHubClient implements GitHubClient {
  getCommit(_oid: GitObjectID): Promise<Commit> {
    return Promise.reject('Method not implemented.');
  }

  getCommitComparison(_base: GitObjectID, _head: GitObjectID): Promise<CommitComparison> {
    return Promise.reject('Method not implemented.');
  }

  getTree(_oid: GitObjectID): Promise<Tree> {
    return Promise.reject('Method not implemented.');
  }

  getBlob(oid: GitObjectID): Promise<Blob> {
    return Promise.reject(`getBlob(${oid}) not implemented`);
  }

  getPullRequest(_pr: number): Promise<PullRequest | null> {
    return Promise.reject('Method not implemented.');
  }

  getPullRequests(_input: PullsQueryInput): Promise<PullsWithPageInfo> {
    return Promise.reject('Method not implemented.');
  }

  getRepoAssignableUsers(_query: string | null): Promise<UserFragment[]> {
    return Promise.reject('Method not implemented.');
  }

  getRepoLabels(_query: string | null): Promise<LabelFragment[]> {
    return Promise.reject('Method not implemented.');
  }

  getStackPullRequests(_prs: number[]): Promise<StackPullRequestFragment[]> {
    return Promise.reject('Method not implemented.');
  }

  addComment(_id: ID, _body: string): Promise<AddCommentMutationData> {
    return Promise.reject('Method not implemented.');
  }

  addLabels(_input: AddLabelsToLabelableInput): Promise<AddLabelsToLabelableMutationData> {
    return Promise.reject('Method not implemented.');
  }

  addPullRequestReview(
    _input: AddPullRequestReviewInput,
  ): Promise<AddPullRequestReviewMutationData> {
    return Promise.reject('Method not implemented.');
  }

  addPullRequestReviewComment(
    _input: AddPullRequestReviewCommentInput,
  ): Promise<AddPullRequestReviewCommentMutationData> {
    return Promise.reject('Method not implemented.');
  }

  removeLabels(
    _input: RemoveLabelsFromLabelableInput,
  ): Promise<RemoveLabelsFromLabelableMutationData> {
    return Promise.reject('Method not implemented.');
  }

  requestReviews(_input: RequestReviewsInput): Promise<RequestReviewsMutationData> {
    return Promise.reject('Method not implemented.');
  }

  submitPullRequestReview(
    _input: SubmitPullRequestReviewInput,
  ): Promise<SubmitPullRequestReviewMutationData> {
    return Promise.reject('Method not implemented.');
  }
}
