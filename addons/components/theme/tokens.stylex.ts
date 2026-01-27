/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';

/*
This file defines theme variables usable in StyleX styles.
 */

// default is dark theme - hybrid of VS Code base with refined accents
export const colors = stylex.defineVars({
  bg: 'var(--background)',
  fg: 'var(--foreground)',
  brightFg: 'white',
  focusBorder: 'var(--focus-border)',

  hoverDarken: 'rgba(255, 255, 255, 0.1)',
  subtleHoverDarken: 'rgba(255, 255, 255, 0.03)',
  highlightFg: '#f0f0f0',

  modifiedFg: '#e2c08d',
  addedFg: '#4ADE80',
  removedFg: '#F87171',
  missingFg: '#60A5FA',

  tooltipBg: 'var(--vscode-editorWidget-background, #252526)',
  tooltipBorder: 'var(--vscode-editorWidget-border, #454545)',

  purple: '#A855F7',
  red: '#EF4444',
  yellow: '#e0d12d',
  orange: '#F97316',
  green: '#22C55E',
  blue: '#60A5FA',
  grey: '#5f6a79',

  signalFg: 'white',
  signalGoodBg: '#22C55E',
  signalMediumBg: '#F97316',
  signalBadBg: '#EF4444',

  errorFg: '#EF4444',
  errorBg: 'rgba(239, 68, 68, 0.15)',

  landFg: 'white',
  landBg: '#22C55E',
  landHoverBg: '#16A34A',
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

  landFg: 'white',
  landBg: '#24853c',
  landHoverBg: '#207134',
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

// Refined color palette - VS Code base with cleaner accents
export const graphiteColors = stylex.defineVars({
  // Layered backgrounds based on VS Code
  bg: 'var(--graphite-bg)',
  bgSubtle: 'var(--graphite-bg-subtle)',
  bgElevated: 'var(--graphite-bg-elevated)',

  // Text hierarchy
  textPrimary: 'var(--graphite-text-primary)',
  textSecondary: 'var(--graphite-text-secondary)',
  textTertiary: 'var(--graphite-text-tertiary)',
  textMuted: 'var(--graphite-text-muted)',

  // Blue accent for interactive elements
  accent: 'var(--graphite-accent)',
  accentHover: 'var(--graphite-accent-hover)',
  accentMuted: 'var(--graphite-accent-muted)',
  accentSubtle: 'var(--graphite-accent-subtle)',

  // Subtle borders
  border: 'var(--graphite-border)',
  borderSubtle: 'var(--graphite-border-subtle)',

  // Hover and selection states
  hoverBg: 'var(--graphite-hover-bg)',
  selectedBg: 'var(--graphite-selected-bg)',

  // Subtle glow for hover effects
  glowColor: 'var(--graphite-glow)',

  // Refined semantic colors
  blue: 'var(--graphite-blue)',
  red: 'var(--graphite-red)',
  redMuted: 'var(--graphite-red-muted)',
  orange: 'var(--graphite-orange)',
  yellow: 'var(--graphite-yellow)',
});

// Layout-specific spacing tokens for breathing room
export const layoutSpacing = stylex.defineVars({
  // Drawer and panel internal padding
  drawerPadding: '16px',

  // Content section spacing
  sectionGap: '12px',

  // Compact internal padding for list items
  itemPadding: '8px',

  // Extra breathing room for prominent sections (middle column)
  prominentPadding: '20px',

  // Gap between columns/sections
  columnGap: '1px',
});
