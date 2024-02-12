/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * This is the prefix used by Monaco for the CSS classes it uses for syntax
 * highlighting. Each token returned by `IGrammar.tokenizeLine2()` has a
 * `number` associated with it. To construct the appropriate CSS class name for
 * color number `n`, do: `${CSS_CLASS_PREFIX}${n}`.
 */
export const CSS_CLASS_PREFIX = 'mtk';

/**
 * Updates the <style> element on the page to define the CSS rules necessary to
 * honor the user's selected theme.
 * @param colorMap as returned by `Registry.getColorMap()` where each value in
 *   the array is a CSS hex value, such as "#AA0000".
 */
export function updateTextMateGrammarCSS(colorMap: string[]): void {
  // Note that if the Monaco editor is used on the page, then we also need to do
  // something like:
  //
  //     const colorMap = cssColors.map(Color.Format.CSS.parseHex);
  //     TokenizationRegistry.setColorMap(colorMap);
  //
  // though that will require loading code from the monaco-editor npm module.

  const css = generateTokensCSSForColorMap(colorMap);
  const style = getOrCreateStyleElementForColorsCSS();
  style.innerHTML = css;
}

let styleElementForTextMateCSS: HTMLStyleElement | null = null;

function getOrCreateStyleElementForColorsCSS(): HTMLStyleElement {
  // If there is an existing <style> element, then overwrite its contents
  // rather than create a new one. (Yes, this means that we support only one
  // theme globally on the page at a time, at least for now.)
  if (styleElementForTextMateCSS != null) {
    return styleElementForTextMateCSS;
  }

  // We want to ensure that our <style> element appears after Monaco's so that
  // we can override some styles it inserted for the default theme.
  styleElementForTextMateCSS = document.createElement('style');

  // If an instance of the Monaco editor is being used on the page, then it will
  // have injected a stylesheet that we need to override. We expect these styles
  // to be in an element with the class name 'monaco-colors' based on:
  // https://github.com/microsoft/vscode/blob/f78d84606cd16d75549c82c68888de91d8bdec9f/src/vs/editor/standalone/browser/standaloneThemeServiceImpl.ts#L206-L214
  //
  // However, .monaco-colors may not have been inserted yet (this could be the
  // case depending on where Monaco is in its own initialization process), so we
  // just append the <style> tag to <body> so that when .monaco-colors is
  // inserted to the <head> tag, our <style> tag will be guaranteed to appear
  // later in the DOM and therefore its styles will take precedence.
  document.body.appendChild(styleElementForTextMateCSS);

  return styleElementForTextMateCSS;
}

/**
 * Adapted from the `generateTokensCSSForColorMap()` implementation in
 * `monaco-editor/esm/vs/editor/common/modes/supports/tokenization.js`.
 * Note that the original takes an Array<Color>, but while the Color class has
 * all sorts of fancy methods like `getRelativeLuminance()` and `blend()`, the
 * only thing this function needs is its `toString()` method, which formats it
 * as a CSS hex value, which is what `registry.getColorMap()` returned in the
 * first place!
 */
function generateTokensCSSForColorMap(cssColors: readonly string[]): string {
  const rules: string[] = [];
  for (let i = 1, len = cssColors.length; i < len; i++) {
    const color = cssColors[i];
    rules[i] = `.${CSS_CLASS_PREFIX}${i} { color: ${color}; }`;
  }
  rules.push(`.${CSS_CLASS_PREFIX}i { font-style: italic; }`);
  rules.push(`.${CSS_CLASS_PREFIX}b { font-weight: bold; }`);
  rules.push(
    `.${CSS_CLASS_PREFIX}u { text-decoration: underline; text-underline-position: under; }`,
  );
  rules.push(`.${CSS_CLASS_PREFIX}s { text-decoration: line-through; }`);
  rules.push(
    `.${CSS_CLASS_PREFIX}s.${CSS_CLASS_PREFIX}u { text-decoration: underline line-through; text-underline-position: under; }`,
  );
  return rules.join('\n');
}
