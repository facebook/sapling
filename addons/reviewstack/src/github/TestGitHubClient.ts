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
import type {MockTree, MockTreeEntry} from './testUtils';
import type {Blob, Commit, GitObjectID, ID, Tree} from './types';

import joinPath from '../joinPath';
import {createTreeEntryFromMock} from './testUtils';

export default class TestGitHubClient implements GitHubClient {
  private trees: Map<GitObjectID, Tree> = new Map();

  constructor(mockTrees: MockTree[]) {
    mockTrees.forEach(mockTree => this.recordEntry(mockTree));
  }

  getCommit(_oid: GitObjectID): Promise<Commit | null> {
    return Promise.resolve(null);
  }

  getCommitComparison(_base: GitObjectID, _head: GitObjectID): Promise<CommitComparison | null> {
    return Promise.resolve(null);
  }

  getTree(oid: GitObjectID): Promise<Tree | null> {
    return Promise.resolve(this.trees.get(oid) ?? null);
  }

  getBlob(_oid: GitObjectID): Promise<Blob | null> {
    return Promise.resolve(null);
  }

  getPullRequest(_pr: number): Promise<PullRequest | null> {
    return Promise.resolve(null);
  }

  getPullRequests(_input: PullsQueryInput): Promise<PullsWithPageInfo | null> {
    return Promise.resolve(null);
  }

  getRepoAssignableUsers(_query: string | null): Promise<UserFragment[]> {
    return Promise.resolve([]);
  }

  getRepoLabels(_query: string | null): Promise<LabelFragment[]> {
    return Promise.resolve([]);
  }

  getStackPullRequests(_prs: number[]): Promise<StackPullRequestFragment[]> {
    return Promise.resolve([]);
  }

  addComment(_id: ID, _body: string): Promise<AddCommentMutationData> {
    return Promise.resolve({});
  }

  addLabels(_input: AddLabelsToLabelableInput): Promise<AddLabelsToLabelableMutationData> {
    return Promise.resolve({});
  }

  addPullRequestReview(
    _input: AddPullRequestReviewInput,
  ): Promise<AddPullRequestReviewMutationData> {
    return Promise.resolve({});
  }

  addPullRequestReviewComment(
    _input: AddPullRequestReviewCommentInput,
  ): Promise<AddPullRequestReviewCommentMutationData> {
    return Promise.resolve({});
  }

  removeLabels(
    _input: RemoveLabelsFromLabelableInput,
  ): Promise<RemoveLabelsFromLabelableMutationData> {
    return Promise.resolve({});
  }

  requestReviews(_input: RequestReviewsInput): Promise<RequestReviewsMutationData> {
    return Promise.resolve({});
  }

  submitPullRequestReview(
    _input: SubmitPullRequestReviewInput,
  ): Promise<SubmitPullRequestReviewMutationData> {
    return Promise.resolve({});
  }

  private recordEntry(mockEntry: MockTreeEntry, basePath = mockEntry.name): void {
    if (mockEntry.type === 'blob') {
      return;
    }

    const {oid} = mockEntry;
    const id = `tree-${oid.slice(0, 3)}`;
    const entries = mockEntry.entries.map(entry => {
      const path = joinPath(basePath, entry.name);

      this.recordEntry(entry, path);
      return createTreeEntryFromMock(entry, path);
    });

    this.trees.set(oid, {id, oid, entries});
  }
}
