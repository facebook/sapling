/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import canExpandLines from './canExpandLines';

describe('canExpandLines', () => {
  const beforeOID = '0123456789abcdef' as Parameters<typeof canExpandLines>[1];

  it('rejects empty and invalid ranges', () => {
    expect(canExpandLines(0, beforeOID)).toBe(false);
    expect(canExpandLines(-1, beforeOID)).toBe(false);
  });

  it('rejects ranges without an original blob', () => {
    expect(canExpandLines(1, null)).toBe(false);
  });

  it('allows non-empty ranges from an original blob', () => {
    expect(canExpandLines(1, beforeOID)).toBe(true);
  });
});
