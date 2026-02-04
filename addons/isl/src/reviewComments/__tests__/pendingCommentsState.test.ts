/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PendingComment} from '../pendingCommentsState';

import {createStore} from 'jotai';
import {
  addPendingComment,
  clearPendingComments,
  getPendingCommentCount,
  pendingCommentsAtom,
  removePendingComment,
} from '../pendingCommentsState';
import {readAtom, setJotaiStore, writeAtom} from '../../jotaiUtils';

describe('pendingCommentsState', () => {
  beforeEach(() => {
    // Create a fresh store for each test to avoid state leaking between tests
    const store = createStore();
    setJotaiStore(store);
    // Clear the atom family cache to ensure clean state
    pendingCommentsAtom.clear();
  });

  describe('addPendingComment', () => {
    it('adds comment with generated ID and timestamp', () => {
      const prNumber = 'test-add-1';

      addPendingComment(prNumber, {
        type: 'inline',
        body: 'Test comment',
        path: 'src/file.ts',
        line: 10,
        side: 'RIGHT',
      });

      const comments = readAtom(pendingCommentsAtom(prNumber));
      expect(comments).toHaveLength(1);
      expect(comments[0].id).toBeDefined();
      expect(typeof comments[0].id).toBe('string');
      expect(comments[0].id.length).toBeGreaterThan(0);
      expect(comments[0].createdAt).toBeDefined();
      expect(typeof comments[0].createdAt).toBe('number');
      expect(comments[0].type).toBe('inline');
      expect(comments[0].body).toBe('Test comment');
      expect(comments[0].path).toBe('src/file.ts');
      expect(comments[0].line).toBe(10);
      expect(comments[0].side).toBe('RIGHT');
    });

    it('preserves existing comments (does not overwrite)', () => {
      const prNumber = 'test-add-2';

      addPendingComment(prNumber, {
        type: 'inline',
        body: 'First comment',
        path: 'file1.ts',
        line: 5,
        side: 'LEFT',
      });

      addPendingComment(prNumber, {
        type: 'inline',
        body: 'Second comment',
        path: 'file2.ts',
        line: 10,
        side: 'RIGHT',
      });

      const comments = readAtom(pendingCommentsAtom(prNumber));
      expect(comments).toHaveLength(2);
      expect(comments[0].body).toBe('First comment');
      expect(comments[1].body).toBe('Second comment');
    });
  });

  describe('removePendingComment', () => {
    it('removes only the specified comment', () => {
      const prNumber = '456';

      // Add multiple comments
      addPendingComment(prNumber, {type: 'inline', body: 'Comment 1', path: 'a.ts', line: 1});
      addPendingComment(prNumber, {type: 'inline', body: 'Comment 2', path: 'b.ts', line: 2});
      addPendingComment(prNumber, {type: 'inline', body: 'Comment 3', path: 'c.ts', line: 3});

      const comments = readAtom(pendingCommentsAtom(prNumber));
      expect(comments).toHaveLength(3);

      // Remove the middle comment
      const commentToRemove = comments[1];
      removePendingComment(prNumber, commentToRemove.id);

      const remaining = readAtom(pendingCommentsAtom(prNumber));
      expect(remaining).toHaveLength(2);
      expect(remaining[0].body).toBe('Comment 1');
      expect(remaining[1].body).toBe('Comment 3');
    });

    it('does nothing when removing non-existent comment', () => {
      const prNumber = '789';

      addPendingComment(prNumber, {type: 'pr', body: 'Test comment'});

      removePendingComment(prNumber, 'non-existent-id');

      const comments = readAtom(pendingCommentsAtom(prNumber));
      expect(comments).toHaveLength(1);
    });
  });

  describe('clearPendingComments', () => {
    it('removes all comments for a PR', () => {
      const prNumber = '111';

      addPendingComment(prNumber, {type: 'inline', body: 'Comment 1', path: 'a.ts', line: 1});
      addPendingComment(prNumber, {type: 'file', body: 'Comment 2', path: 'b.ts'});
      addPendingComment(prNumber, {type: 'pr', body: 'Comment 3'});

      expect(readAtom(pendingCommentsAtom(prNumber))).toHaveLength(3);

      clearPendingComments(prNumber);

      expect(readAtom(pendingCommentsAtom(prNumber))).toHaveLength(0);
    });
  });

  describe('getPendingCommentCount', () => {
    it('returns correct count', () => {
      const prNumber = '222';

      expect(getPendingCommentCount(prNumber)).toBe(0);

      addPendingComment(prNumber, {type: 'inline', body: 'Comment 1', path: 'a.ts', line: 1});
      expect(getPendingCommentCount(prNumber)).toBe(1);

      addPendingComment(prNumber, {type: 'pr', body: 'Comment 2'});
      expect(getPendingCommentCount(prNumber)).toBe(2);

      clearPendingComments(prNumber);
      expect(getPendingCommentCount(prNumber)).toBe(0);
    });
  });

  describe('PR isolation', () => {
    it('different PR numbers have separate comment arrays', () => {
      const pr1 = '100';
      const pr2 = '200';

      addPendingComment(pr1, {type: 'inline', body: 'PR1 comment', path: 'a.ts', line: 1});
      addPendingComment(pr2, {type: 'inline', body: 'PR2 comment', path: 'b.ts', line: 2});
      addPendingComment(pr1, {type: 'pr', body: 'PR1 review'});

      const pr1Comments = readAtom(pendingCommentsAtom(pr1));
      const pr2Comments = readAtom(pendingCommentsAtom(pr2));

      expect(pr1Comments).toHaveLength(2);
      expect(pr2Comments).toHaveLength(1);
      expect(pr1Comments[0].body).toBe('PR1 comment');
      expect(pr2Comments[0].body).toBe('PR2 comment');
    });

    it('clearing one PR does not affect another', () => {
      const pr1 = '300';
      const pr2 = '400';

      addPendingComment(pr1, {type: 'inline', body: 'PR1 comment', path: 'a.ts', line: 1});
      addPendingComment(pr2, {type: 'inline', body: 'PR2 comment', path: 'b.ts', line: 2});

      clearPendingComments(pr1);

      expect(readAtom(pendingCommentsAtom(pr1))).toHaveLength(0);
      expect(readAtom(pendingCommentsAtom(pr2))).toHaveLength(1);
    });
  });

  describe('comment structure', () => {
    it('inline comment has correct structure', () => {
      const prNumber = '500';

      addPendingComment(prNumber, {
        type: 'inline',
        body: 'Inline comment',
        path: 'src/component.tsx',
        line: 42,
        side: 'LEFT',
      });

      const comment = readAtom(pendingCommentsAtom(prNumber))[0];

      expect(comment).toEqual({
        id: expect.any(String),
        type: 'inline',
        body: 'Inline comment',
        path: 'src/component.tsx',
        line: 42,
        side: 'LEFT',
        createdAt: expect.any(Number),
      });
    });

    it('file comment has correct structure (no line/side)', () => {
      const prNumber = '600';

      addPendingComment(prNumber, {
        type: 'file',
        body: 'File-level comment',
        path: 'src/utils.ts',
      });

      const comment = readAtom(pendingCommentsAtom(prNumber))[0];

      expect(comment).toEqual({
        id: expect.any(String),
        type: 'file',
        body: 'File-level comment',
        path: 'src/utils.ts',
        createdAt: expect.any(Number),
      });
      expect(comment.line).toBeUndefined();
      expect(comment.side).toBeUndefined();
    });

    it('pr comment has correct structure (no path/line/side)', () => {
      const prNumber = '700';

      addPendingComment(prNumber, {
        type: 'pr',
        body: 'Overall review comment',
      });

      const comment = readAtom(pendingCommentsAtom(prNumber))[0];

      expect(comment).toEqual({
        id: expect.any(String),
        type: 'pr',
        body: 'Overall review comment',
        createdAt: expect.any(Number),
      });
      expect(comment.path).toBeUndefined();
      expect(comment.line).toBeUndefined();
      expect(comment.side).toBeUndefined();
    });
  });
});
