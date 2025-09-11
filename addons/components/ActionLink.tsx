/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';

const styles = stylex.create({
  button: {
    display: 'flex',
    alignItems: 'center',
    fontSize: '12px',
    color: 'var(--vscode-descriptionForeground)',
    whiteSpace: 'nowrap',
    cursor: 'pointer',
    gap: '5px',
    boxSizing: 'border-box',
    borderBottom: {
      default: '1px solid transparent',
      ':hover': '1px solid var(--vscode-editor-foreground)',
    },
  },
});

export default function ActionLink({
  onClick,
  children,
  title,
}: {
  onClick: () => void;
  title?: string;
  children: React.ReactNode;
}) {
  return (
    <div {...stylex.props(styles.button)} onClick={() => onClick()} title={title}>
      {children}
    </div>
  );
}
