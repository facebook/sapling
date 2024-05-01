/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {processTerminalLines} from '../terminalOutput';

describe('terminalOutput', () => {
  describe('handles \\r', () => {
    function lines(s: string): Array<string> {
      // lines from process output may be flushed at different times, not necessarily only at `\n`
      return s.split(/([\r\n])/);
    }
    it('handles normal lines', () => {
      expect(processTerminalLines(lines('foo\nbar\nbaz'))).toEqual(['foo', 'bar', 'baz']);
      expect(processTerminalLines(lines('foo\nbar\nbaz\n'))).toEqual(['foo', 'bar', 'baz']);
    });

    it('handles \\r\\n as line endings', () => {
      expect(processTerminalLines(lines('foo\r\nbar\r\nbaz\r\n'))).toEqual(['foo', 'bar', 'baz']);
    });

    it('erases earlier parts of lines using \\r', () => {
      expect(
        processTerminalLines(
          lines('foo\nProgress 0%\rProgress 33%\rProgress 66%\rProgress 100%\nDone!\n'),
        ),
      ).toEqual(['foo', 'Progress 100%', 'Done!']);
      expect(
        processTerminalLines(
          lines('foo\nProgress 0%\rProgress 33%\rProgress 66%\rProgress 100%\r\nDone!\n'),
        ),
      ).toEqual(['foo', 'Progress 100%', 'Done!']);
    });

    it('trailing \\r still shows progress', () => {
      expect(processTerminalLines(lines('foo\nProgress 0%\rProgress 33%\rProgress 66%\r'))).toEqual(
        ['foo', 'Progress 66%'],
      );
      expect(
        processTerminalLines(lines('foo\nProgress 0%\rProgress 33%\rProgress 66%\r\n')),
      ).toEqual(['foo', 'Progress 66%']);
    });
  });
});
