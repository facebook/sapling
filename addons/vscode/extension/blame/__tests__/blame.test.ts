/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from 'isl/src/types';

import {getRealignedBlameInfo} from '../blameUtils';

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
});
