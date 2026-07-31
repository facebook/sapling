/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {makeParentVisibilitySource, PARENT_VISIBILITY_VERSION} from '../parentVisibility';

describe('parent visibility source', () => {
  it('registers, validates updates, acknowledges, and disposes subscribers', () => {
    const parentPost = jest.spyOn(window.parent, 'postMessage').mockImplementation(() => {});
    const source = makeParentVisibilitySource(window);
    const changed = jest.fn();
    const subscription = source.onDidChangeVisibility(changed);

    expect(parentPost).toHaveBeenCalledWith(
      {
        type: 'isl/platform/visibility/ready',
        version: PARENT_VISIBILITY_VERSION,
        graphicsContextCount: null,
      },
      '*',
    );

    window.dispatchEvent(
      new MessageEvent('message', {
        source: window.parent,
        data: {
          type: 'isl/platform/visibility/set',
          version: PARENT_VISIBILITY_VERSION,
          visibility: 'hidden',
        },
      }),
    );
    expect(source.getVisibility()).toBe('hidden');
    expect(changed).toHaveBeenCalledWith('hidden');
    expect(parentPost).toHaveBeenCalledWith(
      {
        type: 'isl/platform/visibility/ack',
        version: PARENT_VISIBILITY_VERSION,
        visibility: 'hidden',
      },
      '*',
    );

    window.dispatchEvent(
      new MessageEvent('message', {
        source: window.parent,
        data: {
          type: 'isl/platform/visibility/set',
          version: PARENT_VISIBILITY_VERSION,
          visibility: 'invalid',
        },
      }),
    );
    expect(source.getVisibility()).toBe('hidden');
    expect(changed).toHaveBeenCalledTimes(1);

    subscription.dispose();
    parentPost.mockRestore();
  });
});
