/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type GitHubClient from './GitHubClient';
import type {PullRequest} from './pullRequestTimelineTypes';
import type {PullsQueryInput, PullsWithPageInfo} from './pullsTypes';
import type {CommitComparison} from './restApiTypes';
import type {Blob, Commit, GitObjectID, ID, Tree} from './types';
import type {
  AddCommentMutationData,
  AddLabelsToLabelableInput,
  AddLabelsToLabelableMutationData,
  AddPullRequestReviewInput,
  AddPullRequestReviewMutationData,
  AddPullRequestReviewCommentInput,
  AddPullRequestReviewCommentMutationData,
  AddPullRequestReviewThreadInput,
  AddPullRequestReviewThreadMutationData,
  AddReactionInput,
  AddReactionMutationData,
  ConvertPullRequestToDraftInput,
  ConvertPullRequestToDraftMutationData,
  DeleteIssueCommentInput,
  DeleteIssueCommentMutationData,
  DeletePullRequestReviewCommentInput,
  DeletePullRequestReviewCommentMutationData,
  LabelFragment,
  MarkPullRequestReadyForReviewInput,
  MarkPullRequestReadyForReviewMutationData,
  RemoveLabelsFromLabelableInput,
  RemoveLabelsFromLabelableMutationData,
  RemoveReactionInput,
  RemoveReactionMutationData,
  RequestReviewsInput,
  RequestReviewsMutationData,
  ResolveReviewThreadInput,
  ResolveReviewThreadMutationData,
  StackPullRequestFragment,
  SubmitPullRequestReviewInput,
  SubmitPullRequestReviewMutationData,
  UpdateIssueCommentInput,
  UpdateIssueCommentMutationData,
  UpdatePullRequestInput,
  UpdatePullRequestMutationData,
  UpdatePullRequestReviewCommentInput,
  UpdatePullRequestReviewCommentMutationData,
  UnresolveReviewThreadInput,
  UnresolveReviewThreadMutationData,
  UserFragment,
} from '../generated/graphql';

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

  prefetchTree(_oid: GitObjectID): Promise<void> {
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

  convertPullRequestToDraft(
    _input: ConvertPullRequestToDraftInput,
  ): Promise<ConvertPullRequestToDraftMutationData> {
    return Promise.reject('Method not implemented.');
  }

  markPullRequestReadyForReview(
    _input: MarkPullRequestReadyForReviewInput,
  ): Promise<MarkPullRequestReadyForReviewMutationData> {
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

  addPullRequestReviewThread(
    _input: AddPullRequestReviewThreadInput,
  ): Promise<AddPullRequestReviewThreadMutationData> {
    return Promise.reject('Method not implemented.');
  }

  addReaction(_input: AddReactionInput): Promise<AddReactionMutationData> {
    return Promise.reject('Method not implemented.');
  }

  removeReaction(_input: RemoveReactionInput): Promise<RemoveReactionMutationData> {
    return Promise.reject('Method not implemented.');
  }

  resolveReviewThread(_input: ResolveReviewThreadInput): Promise<ResolveReviewThreadMutationData> {
    return Promise.reject('Method not implemented.');
  }

  unresolveReviewThread(
    _input: UnresolveReviewThreadInput,
  ): Promise<UnresolveReviewThreadMutationData> {
    return Promise.reject('Method not implemented.');
  }

  updateIssueComment(_input: UpdateIssueCommentInput): Promise<UpdateIssueCommentMutationData> {
    return Promise.reject('Method not implemented.');
  }

  updatePullRequest(_input: UpdatePullRequestInput): Promise<UpdatePullRequestMutationData> {
    return Promise.reject('Method not implemented.');
  }

  deleteIssueComment(_input: DeleteIssueCommentInput): Promise<DeleteIssueCommentMutationData> {
    return Promise.reject('Method not implemented.');
  }

  updatePullRequestReviewComment(
    _input: UpdatePullRequestReviewCommentInput,
  ): Promise<UpdatePullRequestReviewCommentMutationData> {
    return Promise.reject('Method not implemented.');
  }

  deletePullRequestReviewComment(
    _input: DeletePullRequestReviewCommentInput,
  ): Promise<DeletePullRequestReviewCommentMutationData> {
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
