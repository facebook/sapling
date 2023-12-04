/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, Hash, SuccessorInfo} from '../../types';

import {BaseDag} from '../base_dag';
import {Dag, REBASE_SUCC_PREFIX} from '../dag';

describe('Dag', () => {
  // Dummy info.
  const date = new Date(42);
  const info: CommitInfo = {
    title: '',
    hash: '',
    parents: [],
    phase: 'draft',
    isHead: false,
    author: '',
    date,
    description: '',
    bookmarks: [],
    remoteBookmarks: [],
    totalFileCount: 0,
    filesSample: [],
  };

  describe('basic queries', () => {
    const dagAbc = new BaseDag().add([
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
      const dag = new Dag()
        .add([
          {...info, hash: 'a', parents: ['z']},
          {...info, hash: 'b', parents: ['a']},
          {...info, hash: 'c', parents: ['b', 'a']},
        ])
        .replaceWith(['c', 'd'], (h, _c) => ({
          ...info,
          hash: h,
          parents: ['b'],
        })).commitDag;
      expect(dag.parentHashes('c')).toEqual(['b']);
      expect(dag.parentHashes('d')).toEqual(['b']);
      expect(dag.childHashes('a').toArray()).toEqual(['b']);
      expect(dag.childHashes('b').toArray()).toEqual(['c', 'd']);
    });
  });

  describe('high-level queries', () => {
    /**
     * A--B--C--F
     *     \   /
     *      D-E--G
     */
    const dag = new Dag().add([
      {...info, hash: 'a', parents: []},
      {...info, hash: 'b', parents: ['a']},
      {...info, hash: 'c', parents: ['b']},
      {...info, hash: 'd', parents: ['b']},
      {...info, hash: 'e', parents: ['d']},
      {...info, hash: 'f', parents: ['c', 'e']},
      {...info, hash: 'g', parents: ['e']},
    ]);

    it('parents()', () => {
      expect(dag.parents('f').toSortedArray()).toEqual(['c', 'e']);
      expect(dag.parents(['b', 'c', 'f']).toSortedArray()).toEqual(['a', 'b', 'c', 'e']);
    });

    it('children()', () => {
      expect(dag.children('b').toSortedArray()).toEqual(['c', 'd']);
      expect(dag.children(['a', 'b', 'd']).toSortedArray()).toEqual(['b', 'c', 'd', 'e']);
    });

    it('ancestors()', () => {
      expect(dag.ancestors('c').toSortedArray()).toEqual(['a', 'b', 'c']);
      expect(dag.ancestors('f').toSortedArray()).toEqual(['a', 'b', 'c', 'd', 'e', 'f']);
      expect(dag.ancestors('g').toSortedArray()).toEqual(['a', 'b', 'd', 'e', 'g']);
      expect(dag.ancestors('f', {within: ['a', 'c', 'd', 'e']}).toSortedArray()).toEqual([
        'c',
        'd',
        'e',
        'f',
      ]);
    });

    it('descendants()', () => {
      expect(dag.descendants('a').toSortedArray()).toEqual(['a', 'b', 'c', 'd', 'e', 'f', 'g']);
      expect(dag.descendants('c').toSortedArray()).toEqual(['c', 'f']);
      expect(dag.descendants('d').toSortedArray()).toEqual(['d', 'e', 'f', 'g']);
      expect(dag.descendants('b', {within: ['c', 'd', 'g']}).toSortedArray()).toEqual([
        'b',
        'c',
        'd',
      ]);
    });

    it('heads()', () => {
      expect(dag.heads(['a', 'b', 'c']).toSortedArray()).toEqual(['c']);
      expect(dag.heads(['d', 'e', 'g']).toSortedArray()).toEqual(['g']);
      expect(dag.heads(['e', 'f', 'g']).toSortedArray()).toEqual(['f', 'g']);
      expect(dag.heads(['c', 'e', 'f']).toSortedArray()).toEqual(['f']);
    });

    it('roots()', () => {
      expect(dag.roots(['a', 'b', 'c']).toSortedArray()).toEqual(['a']);
      expect(dag.roots(['d', 'e', 'g']).toSortedArray()).toEqual(['d']);
      expect(dag.roots(['e', 'f', 'g']).toSortedArray()).toEqual(['e']);
      expect(dag.roots(['c', 'e', 'f']).toSortedArray()).toEqual(['c', 'e']);
    });

    it('range()', () => {
      expect(dag.range('a', 'c').toSortedArray()).toEqual(['a', 'b', 'c']);
      expect(dag.range('a', 'f').toSortedArray()).toEqual(['a', 'b', 'c', 'd', 'e', 'f']);
      expect(dag.range('b', 'g').toSortedArray()).toEqual(['b', 'd', 'e', 'g']);
      expect(dag.range(['a', 'b'], ['a', 'b']).toSortedArray()).toEqual(['a', 'b']);
    });

    it('gca()', () => {
      expect(dag.gca('f', 'g').toSortedArray()).toEqual(['e']);
      expect(dag.gca('f', 'e').toSortedArray()).toEqual(['e']);
      expect(dag.gca('c', 'e').toSortedArray()).toEqual(['b']);
    });

    it('isAncestor()', () => {
      expect(dag.isAncestor('a', 'a')).toBe(true);
      expect(dag.isAncestor('b', 'g')).toBe(true);
      expect(dag.isAncestor('d', 'f')).toBe(true);
      expect(dag.isAncestor('c', 'g')).toBe(false);
      expect(dag.isAncestor('g', 'a')).toBe(false);
    });

    it('supports present()', () => {
      expect(dag.present(['a', 'x']).toSortedArray()).toEqual(['a']);
    });

    it('does not infinite loop on cyclic graphs', () => {
      const dag = new BaseDag().add([
        {hash: 'a', parents: ['b']},
        {hash: 'b', parents: ['c']},
        {hash: 'c', parents: ['a']},
      ]);
      expect(dag.ancestors('b').toSortedArray()).toEqual(['a', 'b', 'c']);
      expect(dag.descendants('b').toSortedArray()).toEqual(['a', 'b', 'c']);
      expect(dag.isAncestor('a', 'c')).toBe(true);
      expect(dag.isAncestor('c', 'a')).toBe(true);
    });
  });

  describe('mutation', () => {
    // mutation: a-->a1-->a2-->a3
    // dag: a1  a2 b.
    const dag = new Dag()
      .add([
        {...info, hash: 'a', successorInfo: {hash: 'a1', type: ''}},
        {...info, hash: 'a1'},
        {...info, hash: 'a2', closestPredecessors: ['a1']},
        {...info, hash: 'a3', closestPredecessors: ['a2']},
        {...info, hash: 'b'},
      ])
      .remove(['a', 'a3']);

    it('followSuccessors()', () => {
      expect(dag.followSuccessors(['a', 'b']).toSortedArray()).toEqual(['a2', 'b']);
      expect(dag.followSuccessors(['a3']).toSortedArray()).toEqual(['a3']);
      expect(dag.followSuccessors(['a1', 'a2']).toSortedArray()).toEqual(['a2']);
    });

    it('successors()', () => {
      expect(dag.successors(['a', 'b']).toSortedArray()).toEqual(['a', 'a1', 'a2', 'b']);
      expect(dag.successors(['a1', 'a2']).toSortedArray()).toEqual(['a1', 'a2']);
    });

    it('picks stack top when following a split', () => {
      // mutation: a->b a->c a->d
      // dag: a  b--d--c.
      const dag = new Dag()
        .add([
          {...info, hash: 'a'},
          {...info, hash: 'b', closestPredecessors: ['a']},
          {...info, hash: 'c', closestPredecessors: ['a'], parents: ['d']},
          {...info, hash: 'd', closestPredecessors: ['a'], parents: ['b']},
        ])
        .remove(['a']);
      // not ['d'] or ['b', 'c', 'd']
      expect(dag.followSuccessors('a').toSortedArray()).toEqual(['c']);
    });
  });

  describe('rebase', () => {
    const succ = (h: Hash): Hash => `${REBASE_SUCC_PREFIX}${h}`;

    it('can break linear stack', () => {
      // a--b--c   rebase -r c -d a
      let dag = new Dag().add([
        {...info, hash: 'a', parents: []},
        {...info, hash: 'b', parents: ['a']},
        {...info, hash: 'c', parents: ['b']},
      ]);
      dag = dag.rebase(['c'], 'a');
      expect(dag.parentHashes('c')).toEqual(['a']);
    });

    it('skips already rebased branches', () => {
      // a--------b            rebase -r c+d+e+f -d b
      //  \        \           e f should not be touched.
      //   c--d     e--f
      let dag = new Dag().add([
        {...info, hash: 'a', parents: [], phase: 'public'},
        {...info, hash: 'b', parents: ['a'], phase: 'public'},
        {...info, hash: 'c', parents: ['a'], phase: 'draft'},
        {...info, hash: 'd', parents: ['c'], phase: 'draft'},
        {...info, hash: 'e', parents: ['b'], phase: 'draft'},
        {...info, hash: 'f', parents: ['e'], phase: 'draft'},
      ]);
      dag = dag.rebase(['c', 'd', 'e', 'f'], 'b');

      // e and f should not be touched
      expect(dag.get('e')?.date).toEqual(date);
      expect(dag.get('f')?.date).toEqual(date);

      // c and d are touched
      expect(dag.get('c')?.date).not.toEqual(date);
      expect(dag.get('d')?.date).not.toEqual(date);

      // check b--e--f and b--c--d
      expect(dag.parentHashes('f')).toEqual(['e']);
      expect(dag.parentHashes('e')).toEqual(['b']);
      expect(dag.parentHashes('d')).toEqual(['c']);
      expect(dag.parentHashes('c')).toEqual(['b']);
    });

    it('handles orphaned commits', () => {
      // a--b  z; rebase -r a -d z; result:
      // a(pred)--b  z--a(succ).
      let dag = new Dag().add([
        {...info, hash: 'z', parents: [], phase: 'public'},
        {...info, hash: 'a', parents: [], phase: 'draft'},
        {...info, hash: 'b', parents: ['a'], phase: 'draft'},
      ]);
      dag = dag.rebase(['a'], 'z');

      // check z--a(succ)
      expect(dag.parentHashes(succ('a'))).toEqual(['z']);
      expect(dag.get(succ('a'))?.date).not.toEqual(date);

      // check a(pred)--b
      expect(dag.parentHashes('b')).toEqual(['a']);
      expect(dag.parentHashes('a')).toEqual([]);
      expect(dag.get('a')?.date).toEqual(date);
      expect(dag.get('b')?.date).toEqual(date);
    });

    it('handles non-continous selection', () => {
      // a--b--c--d--e--f  z; rebase b+c+e+f to z; result:
      // a--b(pred)--c(pred)--d; z--b(succ)--c(succ)--e--f
      let dag = new Dag().add([
        {...info, hash: 'a', parents: []},
        {...info, hash: 'b', parents: ['a']},
        {...info, hash: 'c', parents: ['b']},
        {...info, hash: 'd', parents: ['c']}, // not rebasing
        {...info, hash: 'e', parents: ['d']},
        {...info, hash: 'f', parents: ['e']},
        {...info, hash: 'z', parents: []},
      ]);
      dag = dag.rebase(['b', 'c', 'e', 'f'], 'z');

      // check z--b(succ)--c(succ)--e--f
      expect(dag.parentHashes('f')).toEqual(['e']);
      expect(dag.parentHashes('e')).toEqual([succ('c')]);
      expect(dag.parentHashes(succ('c'))).toEqual([succ('b')]);
      expect(dag.parentHashes(succ('b'))).toEqual(['z']);

      // check a--b(pred)--c(pred)--c--d
      expect(dag.parentHashes('c')).toEqual(['b']);
      expect(dag.parentHashes('b')).toEqual(['a']);
      expect(dag.childHashes('d').toArray()).toEqual([]);

      // succ and pred info
      expect(dag.get('b')?.successorInfo?.hash).toEqual(succ('b'));
      expect(dag.get('c')?.successorInfo?.hash).toEqual(succ('c'));
      expect(dag.get(succ('b'))?.closestPredecessors).toEqual(['b']);
      expect(dag.get(succ('c'))?.closestPredecessors).toEqual(['c']);

      // orphaned and obsoleted b--c--d are not touched
      expect(dag.get('b')?.date).toEqual(date);
      expect(dag.get('c')?.date).toEqual(date);
      expect(dag.get('d')?.date).toEqual(date);
    });

    it('cleans up obsoleted commits', () => {
      // a--b--c--f    rebase -r f -d z
      //  \      /     b, c, d, e are obsoleted
      //   -d--e-      b is head
      // z             check: c, d, e are removed
      const successorInfo: SuccessorInfo = {hash: 'z', type: 'rewrite'};
      let dag = new Dag().add([
        {...info, hash: 'z', parents: [], phase: 'public'},
        {...info, hash: 'a', parents: [], phase: 'draft'},
        {...info, hash: 'b', parents: ['a'], phase: 'draft', date, successorInfo, isHead: true},
        {...info, hash: 'c', parents: ['b'], phase: 'draft', date, successorInfo},
        {...info, hash: 'd', parents: ['a'], phase: 'draft', date, successorInfo},
        {...info, hash: 'e', parents: ['d'], phase: 'draft', date, successorInfo},
        {...info, hash: 'f', parents: ['c', 'e'], phase: 'draft', date},
      ]);
      dag = dag.rebase(['f'], 'z');
      expect(['b', 'c', 'd', 'e'].filter(h => dag.has(h))).toEqual(['b']);
    });
  });
});
