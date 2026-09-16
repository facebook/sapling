/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {suggestionBody} from '../GitHubReviewCommentsProvider';

describe('GitHub review suggestions', () => {
  it('copies selected lines into a GitHub suggestion block', () => {
    expect(suggestionBody('const one = 1;\nconst two = 2;')).toBe(
      '```suggestion\nconst one = 1;\nconst two = 2;\n```',
    );
  });

  it('preserves an existing comment before the suggestion', () => {
    expect(suggestionBody('return result;', 'Please simplify this.')).toBe(
      'Please simplify this.\n\n```suggestion\nreturn result;\n```',
    );
  });
});
