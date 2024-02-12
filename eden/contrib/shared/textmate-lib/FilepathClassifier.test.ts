/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  grammars,
  languages,
} from '../../reviewstack/src/generated/textmate/TextMateGrammarManifest';
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

describe('getDisplayNameForLanguageId', () => {
  it('verify tags from fenced code blocks get mapped to a human-readable name', () => {
    const classifier = new FilepathClassifier(grammars, languages);
    const getDisplayNameForLanguageId = (alias: string) =>
      classifier.getDisplayNameForLanguageId(alias);
    expect(getDisplayNameForLanguageId('')).toBe('');
    expect(getDisplayNameForLanguageId('cpp')).toBe('C++');
    expect(getDisplayNameForLanguageId('csharp')).toBe('C#');
    expect(getDisplayNameForLanguageId('fsharp')).toBe('F#');
    expect(getDisplayNameForLanguageId('javascript')).toBe('JavaScript');
    expect(getDisplayNameForLanguageId('js')).toBe('JavaScript');
    expect(getDisplayNameForLanguageId('kotlin')).toBe('Kotlin');
    expect(getDisplayNameForLanguageId('objective-c')).toBe('Objective-C');
    expect(getDisplayNameForLanguageId('php')).toBe('Hack');
    expect(getDisplayNameForLanguageId('py')).toBe('Python');
    expect(getDisplayNameForLanguageId('python')).toBe('Python');
    expect(getDisplayNameForLanguageId('rs')).toBe('Rust');
    expect(getDisplayNameForLanguageId('rust')).toBe('Rust');
    expect(getDisplayNameForLanguageId('swift')).toBe('Swift');
  });
});
