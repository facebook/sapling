/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Dag} from '../dag';

describe('Dag', () => {
  describe('basic queries', () => {
    const dagAbc = new Dag().add([
      {hash: 'a', parents: ['z']},
      {hash: 'b', parents: ['a']},
      {hash: 'c', parents: ['b', 'a']},
    ]);

    it('maintains parent<->child mappings', () => {
      const dag = dagAbc;
      expect(dag.parentHashes('a')).toEqual([]);
      expect(dag.parentHashes('b')).toEqual(['a']);
      expect(dag.parentHashes('c')).toEqual(['b', 'a']);
      expect(dag.parentHashes('d')).toEqual([]);
      expect(dag.parentHashes('z')).toEqual([]);
      expect(dag.childHashes('a').toArray()).toEqual(['b', 'c']);
      expect(dag.childHashes('b').toArray()).toEqual(['c']);
      expect(dag.childHashes('c').toArray()).toEqual([]);
      expect(dag.childHashes('d').toArray()).toEqual([]);
      expect(dag.childHashes('z').toArray()).toEqual([]);
    });

    it('maintains parent<->child mappings after remove()', () => {
      const dag = dagAbc.remove(['b']);
      expect(dag.parentHashes('c')).toEqual(['a']);
      expect(dag.childHashes('a').toArray()).toEqual(['c']);
      expect(dag.parentHashes('b')).toEqual([]);
      expect(dag.childHashes('b').toArray()).toEqual([]);
    });

    it('removes conflicted commits', () => {
      const dag = dagAbc.add([{hash: 'c', parents: []}]);
      expect(dag.parentHashes('c')).toEqual([]);
      expect(dag.childHashes('b').toArray()).toEqual([]);
    });

    it('supports replaceWith()', () => {
      const dag = dagAbc.replaceWith(['c', 'd'], (h, _c) => ({hash: h, parents: ['b']}));
      expect(dag.parentHashes('c')).toEqual(['b']);
      expect(dag.parentHashes('d')).toEqual(['b']);
      expect(dag.childHashes('a').toArray()).toEqual(['b']);
      expect(dag.childHashes('b').toArray()).toEqual(['c', 'd']);
    });
  });
});
