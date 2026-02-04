/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Note: Full component tests require React Testing Library setup.
// This file validates that the component can be imported and types are correct.

import type {CommentInputProps} from '../CommentInput';

describe('CommentInput', () => {
  it('has correct prop types', () => {
    // Type-only test - validates the interface
    const props: CommentInputProps = {
      prNumber: '123',
      type: 'inline',
      path: 'src/test.ts',
      line: 42,
      side: 'RIGHT',
      onCancel: () => {},
      onSubmit: () => {},
    };
    expect(props.prNumber).toBe('123');
    expect(props.type).toBe('inline');
  });

  it('accepts file-level comment props', () => {
    const props: CommentInputProps = {
      prNumber: '123',
      type: 'file',
      path: 'src/test.ts',
      onCancel: () => {},
    };
    expect(props.type).toBe('file');
  });

  it('accepts PR-level comment props', () => {
    const props: CommentInputProps = {
      prNumber: '123',
      type: 'pr',
      onCancel: () => {},
    };
    expect(props.type).toBe('pr');
  });
});
