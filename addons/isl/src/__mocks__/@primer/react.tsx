/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export const Box = (p: React.PropsWithChildren<{as?: 'tr' | 'td'; onClick?: () => unknown}>) => {
  if (p.as === 'tr') {
    return <tr onClick={p.onClick}>{p.children}</tr>;
  } else if (p.as === 'td') {
    return <td onClick={p.onClick}>{p.children}</td>;
  }
  return <div onClick={p.onClick}>{p.children}</div>;
};
export const Text = (p: React.PropsWithChildren) => <div>{p.children}</div>;
export const BaseStyles = (p: React.PropsWithChildren) => <div>{p.children}</div>;
export const ThemeProvider = (p: React.PropsWithChildren) => <div>{p.children}</div>;
export const Spinner = () => <div />;

export function useTheme() {
  return {};
}
