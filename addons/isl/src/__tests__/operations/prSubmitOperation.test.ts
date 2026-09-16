/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {GithubUICodeReviewProvider} from '../../codeReview/github/github';
import {COMMIT} from '../../testUtils';
import {exactRevset, succeedableRevset} from '../../types';

const githubSystem = {
  type: 'github' as const,
  owner: 'owner',
  repo: 'repo',
  hostname: 'github.com',
};

describe('PrSubmitOperation', () => {
  const provider = new GithubUICodeReviewProvider(githubSystem, 'pr');

  it('submits a selected commit with reviewers', () => {
    const commit = COMMIT('abc123', 'Selected commit', 'parent');
    const operation = provider.submitOperation([commit], {
      draft: true,
      reviewers: ['alice', 'bob'],
    });

    expect(operation.getArgs()).toEqual([
      'pr',
      'submit',
      '--config',
      'github.submit-to-upstream=false',
      '--draft',
      '--rev',
      succeedableRevset('abc123'),
      '--reviewer',
      'alice',
      '--reviewer',
      'bob',
    ]);
  });

  it('resolves the current commit when the head may change first', () => {
    const operation = provider.submitOperation([], {draft: true});

    expect(operation.getArgs()).toEqual([
      'pr',
      'submit',
      '--config',
      'github.submit-to-upstream=false',
      '--draft',
      '--rev',
      exactRevset('.'),
    ]);
  });

  it('submits the complete current stack', () => {
    const operation = provider.submitOperation([], {
      draft: false,
      submitStack: true,
      reviewers: ['alice'],
    });

    expect(operation.getArgs()).toEqual([
      'pr',
      'submit',
      '--config',
      'github.submit-to-upstream=false',
      '--stack',
      '--reviewer',
      'alice',
    ]);
  });
});
