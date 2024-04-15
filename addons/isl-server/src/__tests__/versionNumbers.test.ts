/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {compareVersions, parseVersionParts} from '../versionNumbers';

describe('version numbers', () => {
  describe('parseVersionParts', () => {
    it('parses version parts', () => {
      expect(parseVersionParts('1.2.3')).toEqual([1, 2, 3]);
      expect(parseVersionParts('1.2')).toEqual([1, 2]);
      expect(parseVersionParts('V0.10')).toEqual([0, 10]);
    });

    it('ignores extra non-numerics', () => {
      expect(parseVersionParts('V1')).toEqual([1]);
      expect(parseVersionParts('V1.1-alpha')).toEqual([1, 1]);
    });
  });

  describe('compareVersions', () => {
    it('compares version parts', () => {
      expect(compareVersions(parseVersionParts('1.2'), parseVersionParts('1.3'))).toEqual(-1);
      expect(compareVersions(parseVersionParts('1.3'), parseVersionParts('1.2'))).toEqual(1);
      expect(compareVersions(parseVersionParts('1.2'), parseVersionParts('1.2'))).toEqual(0);
    });

    it('compares in order', () => {
      expect(compareVersions(parseVersionParts('3.0.0'), parseVersionParts('2.99.999'))).toEqual(1);
      expect(compareVersions(parseVersionParts('3.2.0'), parseVersionParts('3.1.999'))).toEqual(1);
    });

    it('uses integer comparison', () => {
      expect(compareVersions(parseVersionParts('1.10'), parseVersionParts('1.2'))).toEqual(1);
    });

    it('handles different lengths', () => {
      expect(compareVersions(parseVersionParts('1.3.1'), parseVersionParts('1.3'))).toEqual(1);
      expect(compareVersions(parseVersionParts('1.2.1'), parseVersionParts('1.3'))).toEqual(-1);
    });

    it('1 is less than 1.0', () => {
      expect(compareVersions(parseVersionParts('3'), parseVersionParts('3.0'))).toEqual(-1);
    });

    it("extra non-numerics don't affect it", () => {
      expect(compareVersions(parseVersionParts('v3.1-alpha'), parseVersionParts('3.0'))).toEqual(1);
      expect(compareVersions(parseVersionParts('v3.1-alpha'), parseVersionParts('3.1'))).toEqual(0);
    });
  });
});
