/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {getFoldableRange} from '../fold';
import {getCommitTree, makeTreeMap} from '../getCommitTree';
import {COMMIT} from '../testUtils';

describe('fold', () => {
  describe('getFoldableRange', () => {
    const COMMITS = [
      COMMIT('d', 'Commit D', 'c'),
      COMMIT('c', 'Commit C', 'b'),
      COMMIT('b', 'Commit B', 'a'),
      COMMIT('a', 'Commit A', '1'),
      COMMIT('1', 'base', '2', {phase: 'public'}),
    ];
    const [, CC, CB, CA] = COMMITS;

    it('get correct selection', () => {
      expect(
        getFoldableRange(new Set(['a', 'b', 'c']), makeTreeMap(getCommitTree(COMMITS))),
      ).toEqual([CA, CB, CC]);
    });

    it('does not care about selection order', () => {
      expect(
        getFoldableRange(new Set(['b', 'a', 'c']), makeTreeMap(getCommitTree(COMMITS))),
      ).toEqual([CA, CB, CC]);
      expect(
        getFoldableRange(new Set(['c', 'b', 'a']), makeTreeMap(getCommitTree(COMMITS))),
      ).toEqual([CA, CB, CC]);
    });

    it('fails for singular selection', () => {
      expect(getFoldableRange(new Set(['a']), makeTreeMap(getCommitTree(COMMITS)))).toEqual(
        undefined,
      );
    });

    it('fails for public commits', () => {
      expect(
        getFoldableRange(new Set(['1', 'a', 'b', 'c']), makeTreeMap(getCommitTree(COMMITS))),
      ).toEqual(undefined);
    });

    it('fails for non-contiguous selections', () => {
      expect(getFoldableRange(new Set(['a', 'c']), makeTreeMap(getCommitTree(COMMITS)))).toEqual(
        undefined,
      );
    });

    it('fails if there are branches in the middle of the range', () => {
      const COMMITS = [
        COMMIT('d', 'Commit D', 'c'),
        COMMIT('e', 'Commit E', 'b'),
        COMMIT('c', 'Commit C', 'b'),
        COMMIT('b', 'Commit B', 'a'),
        COMMIT('a', 'Commit A', '1'),
        COMMIT('1', 'base', '2', {phase: 'public'}),
      ];
      expect(
        getFoldableRange(new Set(['a', 'b', 'c']), makeTreeMap(getCommitTree(COMMITS))),
      ).toEqual(undefined);
    });

    it('the top of the stack may have multiple children', () => {
      const COMMITS = [
        COMMIT('e', 'Commit E', 'c'),
        COMMIT('d', 'Commit D', 'c'),
        COMMIT('c', 'Commit C', 'b'),
        COMMIT('b', 'Commit B', 'a'),
        COMMIT('a', 'Commit A', '1'),
        COMMIT('1', 'base', '2', {phase: 'public'}),
      ];
      const [, , CC, CB, CA] = COMMITS;
      expect(
        getFoldableRange(new Set(['a', 'b', 'c']), makeTreeMap(getCommitTree(COMMITS))),
      ).toEqual([CA, CB, CC]);
    });
  });
});
