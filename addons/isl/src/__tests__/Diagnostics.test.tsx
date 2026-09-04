/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UseUncommittedSelection} from '../partialSelection';
import type {DiagnosticAllowlist} from '../types';

import {confirmNoBlockingDiagnostics, isBlockingDiagnostic} from '../Diagnostics';
import platform from '../platform';
import {expectMessageSentToServer, resetTestMessages} from '../testUtils';

const range = {startLine: 1, startCol: 1, endLine: 1, endCol: 1};

describe('Diagnostics', () => {
  describe('isBlockingDiagnostic', () => {
    it('handles allowlist', () => {
      const policy: DiagnosticAllowlist = new Map([
        ['error', new Map([['foo', {allow: new Set(['code1'])}]])],
      ]);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'error',
            source: 'foo',
            code: 'code1',
          },
          policy,
        ),
      ).toBe(true);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'error',
            source: 'foo',
            code: 'code3',
          },
          policy,
        ),
      ).toBe(false);
    });

    it('handles blocklist', () => {
      const policy: DiagnosticAllowlist = new Map([
        ['error', new Map([['foo', {block: new Set(['code1'])}]])],
      ]);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'error',
            source: 'foo',
            code: 'code3',
          },
          policy,
        ),
      ).toBe(true);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'error',
            source: 'foo',
            code: 'code1',
          },
          policy,
        ),
      ).toBe(false);
    });

    it('handles wildcard', () => {
      const policy: DiagnosticAllowlist = new Map([
        ['error', new Map([['foo', {block: new Set([])}]])],
      ]);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'error',
            source: 'foo',
            code: 'code1',
          },
          policy,
        ),
      ).toBe(true);
    });

    it('ignores non-errors', () => {
      const policy: DiagnosticAllowlist = new Map([
        ['error', new Map([['foo', {block: new Set([])}]])],
      ]);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'hint',
            source: 'foo',
            code: 'code1',
          },
          policy,
        ),
      ).toBe(false);
    });

    it('accepts warnings', () => {
      const policy: DiagnosticAllowlist = new Map([
        ['warning', new Map([['foo', {block: new Set([])}]])],
      ]);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'warning',
            source: 'foo',
            code: 'code1',
          },
          policy,
        ),
      ).toBe(true);
    });

    it('handles undefined source', () => {
      const policy: DiagnosticAllowlist = new Map([
        ['warning', new Map([['undefined', {block: new Set(['foo'])}]])],
      ]);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'warning',
            source: undefined,
            code: 'foo',
          },
          policy,
        ),
      ).toBe(false);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'warning',
            source: undefined,
            code: 'something_else',
          },
          policy,
        ),
      ).toBe(true);
    });

    it('handles undefined code', () => {
      const policy: DiagnosticAllowlist = new Map([
        ['warning', new Map([['foo', {block: new Set(['undefined'])}]])],
      ]);
      expect(
        isBlockingDiagnostic(
          {
            message: 'sample',
            range,
            severity: 'warning',
            source: 'foo',
            code: undefined,
          },
          policy,
        ),
      ).toBe(false);
    });
  });

  describe('confirmNoBlockingDiagnostics', () => {
    const selection = {
      isFullyOrPartiallySelected: () => true,
    } as unknown as UseUncommittedSelection;
    const originalPlatformName = platform.platformName;

    beforeEach(() => {
      jest.useFakeTimers();
      resetTestMessages();
      (platform as {platformName: string}).platformName = 'vscode';
    });

    afterEach(() => {
      (platform as {platformName: string}).platformName = originalPlatformName;
      jest.useRealTimers();
    });

    it('does not block submitting when the diagnostics reply never arrives', async () => {
      // Deliberately never simulate a 'platform/gotDiagnostics' reply.
      const result = confirmNoBlockingDiagnostics(selection);
      await jest.advanceTimersByTimeAsync(60_000);

      // Guards against passing vacuously via an early return before the check.
      expectMessageSentToServer({type: 'platform/checkForDiagnostics', paths: []});
      await expect(result).resolves.toBe(true);
    });
  });
});
