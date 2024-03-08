/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';

/*
This file defines theme variables useable in StyleX styles.
 */

// default is dark theme
export const colors = stylex.defineVars({
  bg: 'var(--background)',
  fg: 'var(--foreground)',
  brightFg: 'white',
  focusBorder: 'var(--focus-border)',

  hoverDarken: 'rgba(255, 255, 255, 0.1)',
  subtleHoverDarken: 'rgba(255, 255, 255, 0.03)',
  highlightFg: '#f0f0f0',

  modifiedFg: '#e2c08d',
  addedFg: '#73c991',
  removedFg: '#f3674f',
  missingFg: '#b4eaed',

  tooltipBg: 'var(--vscode-editorWidget-background, #252526)',
  tooltipBorder: 'var(--vscode-editorWidget-border, #454545)',

  purple: '#713fc8',
  red: '#cf222e',
  yellow: '#e0d12d',
  orange: '#dd7c26',
  green: '#2da44e',
  blue: '#007acc',
  grey: '#5f6a79',

  signalFg: 'white',
  signalGoodBg: '#2da44e',
  signalMediumBg: '#e0d12d',
  signalBadBg: '#cf222e',

  errorFg: '#f3674f',
  errorBg: '#f3674f20',
});

// if using a light theme, we apply a stylex theme to override color variables above
export const light = stylex.createTheme(colors, {
  bg: 'var(--background)',
  fg: 'var(--foreground)',
  brightFg: 'black',
  focusBorder: 'var(--focus-border)',

  hoverDarken: 'rgba(0, 0, 0, 0.1)',
  subtleHoverDarken: 'rgba(0, 0, 0, 0.03)',
  highlightFg: '#2a2a2a',

  modifiedFg: '#895503',
  addedFg: '#007100',
  removedFg: '#ad0707',
  missingFg: '#418c91',

  tooltipBg: 'var(--vscode-editorWidget-background, #f3f3f3)',
  tooltipBorder: 'var(--vscode-editorWidget-border, #c8c8c8)',

  purple: '#713fc8',
  red: '#cf222e',
  yellow: '#e0d12d',
  orange: '#dd7c26',
  green: '#2da44e',
  blue: '#007acc',
  grey: '#5f6a79',

  signalFg: 'white',
  signalGoodBg: '#2da44e',
  signalMediumBg: '#e0d12d',
  signalBadBg: '#cf222e',

  errorFg: '#e35941ff',
  errorBg: '#e3594120',
});

export const spacing = stylex.defineVars({
  none: '0px',
  quarter: '2.5px',
  half: '5px',
  pad: '10px',
  double: '20px',
  xlarge: '32px',
  xxlarge: '48px',
  xxxlarge: '96px',
});

export const radius = stylex.defineVars({
  small: '2.5px',
  round: '5px',
  extraround: '5px',
  full: '50%',
});

export const font = stylex.defineVars({
  smaller: '80%',
  small: '90%',
  normal: '100%',
  big: '110%',
  bigger: '120%',
});
