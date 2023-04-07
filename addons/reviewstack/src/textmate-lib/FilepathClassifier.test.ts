/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {grammars, languages} from '../generated/textmate/TextMateGrammarManifest';
import FilepathClassifier from './FilepathClassifier';

describe('findScopeNameForPath', () => {
  test('map paths to scope names', () => {
    const classifier = new FilepathClassifier(grammars, languages);
    const findScopeNameForPath = (path: string) => classifier.findScopeNameForPath(path);
    expect(findScopeNameForPath('foo/BUCK')).toBe(null);
    expect(findScopeNameForPath('foo/Bar.php')).toBe('source.hack');
    expect(findScopeNameForPath('foo/Bar.java')).toBe('source.java');
    expect(findScopeNameForPath('foo/bar.js')).toBe('source.js');
    expect(findScopeNameForPath('foo/Makefile')).toBe('source.makefile');
    expect(findScopeNameForPath('foo/bar.py')).toBe('source.python');
    expect(findScopeNameForPath('foo/CHANGELOG.md')).toBe('text.html.markdown');
  });
});

describe('findScopeNameForAlias', () => {
  test('verify amended aliases are mapped correctly', () => {
    const classifier = new FilepathClassifier(grammars, languages);
    const findScopeNameForAlias = (alias: string) => classifier.findScopeNameForAlias(alias);
    expect(findScopeNameForAlias('rs')).toBe('source.rust');
  });
});
