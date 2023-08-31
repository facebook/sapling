/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from 'isl-server/src/Repository';
import type {CommitInfo} from 'isl/src/types';

import {getDiffBlameHoverMarkup} from '../blameHover';
import {getRealignedBlameInfo, shortenAuthorName} from '../blameUtils';
import {GitHubCodeReviewProvider} from 'isl-server/src/github/githubCodeReviewProvider';
import {mockLogger} from 'shared/testUtils';

describe('blame', () => {
  describe('getRealignedBlameInfo', () => {
    it('realigns blame', () => {
      const person1 = {author: 'person1', date: new Date('2020-01-01'), hash: 'A'} as CommitInfo;
      const person2 = {author: 'person2', date: new Date('2021-01-01'), hash: 'B'} as CommitInfo;
      const person3 = {author: 'person3', date: new Date('2022-01-01'), hash: 'C'} as CommitInfo;

      const blame: Array<[string, CommitInfo]> = [
        /* A  - person1 */ ['A\n', person1],
        /* B  - person2 */ ['B\n', person2],
        /* C  - person3 */ ['C\n', person3],
        /* D  - person1 */ ['D\n', person1],
        /* E  - person2 */ ['E\n', person2],
        /* F  - person3 */ ['F\n', person3],
        /* G  - person1 */ ['G\n', person1],
      ];

      const after = `\
A
C
D
hi
hey
E
F!
G
`;

      const expected = [
        /* A   - person1 */ [expect.anything(), person1],
        /* C   - person3 */ [expect.anything(), person3],
        /* D   - person1 */ [expect.anything(), person1],
        /* hi  - (you)   */ [expect.anything(), undefined],
        /* hey - (you)   */ [expect.anything(), undefined],
        /* E   - person2 */ [expect.anything(), person2],
        /* F!  - (you)   */ [expect.anything(), undefined],
        /* G   - person1 */ [expect.anything(), person1],
      ];

      const result = getRealignedBlameInfo(blame, after);
      expect(result).toEqual(expected);
    });
  });

  describe('blame hover', () => {
    const mockRepo = {
      codeReviewProvider: new GitHubCodeReviewProvider(
        {type: 'github', owner: 'facebook', repo: 'sapling', hostname: 'github.com'},
        mockLogger,
      ),
    } as unknown as Repository;
    const mockCommit = {
      hash: 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
      date: new Date(),
      author: 'person',
      title: 'My cool PR',
    } as unknown as CommitInfo;
    it('renders attachec PR links', () => {
      expect(
        getDiffBlameHoverMarkup(mockRepo, {
          ...mockCommit,
          date: new Date(),
          description: 'added some stuff',
          diffId: '1234',
        } as unknown as CommitInfo),
      ).toEqual(
        `\
**person** - [#1234](https://github.com/facebook/sapling/pull/1234) (just now)

**My cool PR**


added some stuff`,
      );
    });

    it('renders detected PR links', () => {
      expect(
        getDiffBlameHoverMarkup(mockRepo, {
          ...mockCommit,
          description: 'added some stuff in #1234',
        } as unknown as CommitInfo),
      ).toEqual(
        `\
**person** - [#1234](https://github.com/facebook/sapling/pull/1234) (just now)

**My cool PR**


added some stuff in #1234`,
      );
    });

    it('falls back to commit hash', () => {
      expect(
        getDiffBlameHoverMarkup(mockRepo, {
          ...mockCommit,
          description: 'added some stuff',
        } as unknown as CommitInfo),
      ).toEqual(
        `\
**person** - [\`a1b2c3d4e5f6\`](https://github.com/facebook/sapling/commit/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2) (just now)

**My cool PR**


added some stuff`,
      );
    });
  });
});

describe('blame utils', () => {
  describe('shortenAuthorName', () => {
    it('removes email for inline display', () => {
      expect(shortenAuthorName('John Smith john@example.com')).toEqual('John Smith');
      expect(shortenAuthorName('John Smith <john@example.com>')).toEqual('John Smith');
    });

    it('shows email if no name is given', () => {
      expect(shortenAuthorName('john@example.com')).toEqual('john@example.com');
    });
  });
});
