/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, Hash} from '../../types';
import type {HashWithParents} from '../dag';

import {Dag, REBASE_SUCC_PREFIX} from '../dag';

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

  describe('high-level queries', () => {
    /**
     * A--B--C--F
     *     \   /
     *      D-E--G
     */
    const dag = new Dag().add([
      {hash: 'a', parents: []},
      {hash: 'b', parents: ['a']},
      {hash: 'c', parents: ['b']},
      {hash: 'd', parents: ['b']},
      {hash: 'e', parents: ['d']},
      {hash: 'f', parents: ['c', 'e']},
      {hash: 'g', parents: ['e']},
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

    it('does not infinite loop on cyclic graphs', () => {
      const dag = new Dag().add([
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

  describe('rebase', () => {
    const succ = (h: Hash): Hash => `${REBASE_SUCC_PREFIX}${h}`;
    const date = new Date(42);

    it('can break linear stack', () => {
      // a--b--c   rebase -r c -d a
      let dag = new Dag<Partial<CommitInfo> & HashWithParents>().add([
        {hash: 'a', parents: [], phase: 'public', date},
        {hash: 'b', parents: ['a'], phase: 'draft', date},
        {hash: 'c', parents: ['b'], phase: 'draft', date},
      ]);
      dag = dag.rebase(['c'], 'a');
      expect(dag.parentHashes('c')).toEqual(['a']);
    });

    it('skips already rebased branches', () => {
      // a--------b            rebase -r c+d+e+f -d b
      //  \        \           e f should not be touched.
      //   c--d     e--f
      let dag = new Dag<Partial<CommitInfo> & HashWithParents>().add([
        {hash: 'a', parents: [], phase: 'public'},
        {hash: 'b', parents: ['a'], phase: 'public'},
        {hash: 'c', parents: ['a'], phase: 'draft', date},
        {hash: 'd', parents: ['c'], phase: 'draft', date},
        {hash: 'e', parents: ['b'], phase: 'draft', date},
        {hash: 'f', parents: ['e'], phase: 'draft', date},
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
      let dag = new Dag<Partial<CommitInfo> & HashWithParents>().add([
        {hash: 'z', parents: [], phase: 'public', date},
        {hash: 'a', parents: [], phase: 'draft', date},
        {hash: 'b', parents: ['a'], phase: 'draft', date},
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
      let dag = new Dag<Partial<CommitInfo> & HashWithParents>().add([
        {hash: 'a', parents: [], phase: 'draft'},
        {hash: 'b', parents: ['a'], phase: 'draft', date},
        {hash: 'c', parents: ['b'], phase: 'draft', date},
        {hash: 'd', parents: ['c'], phase: 'draft', date}, // not rebasing
        {hash: 'e', parents: ['d'], phase: 'draft'},
        {hash: 'f', parents: ['e'], phase: 'draft'},
        {hash: 'z', parents: []},
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
  });
});
