/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {T} from './i18n';
import {copyAndShowToast} from './toast';
import {showModal} from './useModal';

const fadeIn = stylex.keyframes({
  from: {opacity: 0, transform: 'translateY(4px)'},
  to: {opacity: 1, transform: 'translateY(0)'},
});

const styles = stylex.create({
  container: {
    display: 'flex',
    flexDirection: 'column',
    gap: '16px',
    padding: '4px 4px 8px',
    minWidth: '400px',
    maxWidth: '560px',
  },
  hint: {
    fontSize: '12.5px',
    color: 'var(--foreground-sub)',
    lineHeight: '1.5',
    animation: `${fadeIn} 0.3s ease-out`,
  },
  commandBlock: {
    display: 'flex',
    flexDirection: 'column',
    borderRadius: '8px',
    overflow: 'hidden',
    border: '1px solid var(--graphite-border, rgba(255, 255, 255, 0.08))',
    boxShadow: '0 2px 8px rgba(0, 0, 0, 0.2), inset 0 1px 0 rgba(255, 255, 255, 0.03)',
    animation: `${fadeIn} 0.35s ease-out`,
  },
  commandHeader: {
    display: 'flex',
    alignItems: 'center',
    gap: '6px',
    padding: '6px 12px',
    backgroundColor: 'rgba(255, 255, 255, 0.03)',
    borderBottom: '1px solid var(--graphite-border, rgba(255, 255, 255, 0.06))',
    fontSize: '10px',
    fontWeight: 600,
    letterSpacing: '0.05em',
    textTransform: 'uppercase' as const,
    color: 'var(--foreground-sub)',
  },
  terminalDot: {
    width: '6px',
    height: '6px',
    borderRadius: '50%',
    backgroundColor: 'var(--graphite-accent, #4a90e2)',
    opacity: 0.7,
  },
  commandRow: {
    display: 'flex',
    alignItems: 'center',
    gap: '10px',
    padding: '10px 12px',
    backgroundColor: 'var(--tooltip-background, #1a1a1a)',
  },
  prompt: {
    color: 'var(--graphite-accent, #4a90e2)',
    fontFamily:
      'var(--monospace-fontFamily, "SF Mono", "Fira Code", "Cascadia Code", Menlo, monospace)',
    fontSize: '13px',
    fontWeight: 600,
    flexShrink: 0,
    userSelect: 'none',
  },
  commandText: {
    flex: 1,
    fontFamily:
      'var(--monospace-fontFamily, "SF Mono", "Fira Code", "Cascadia Code", Menlo, monospace)',
    fontSize: '13px',
    color: 'var(--tooltip-foreground, #e0e0e0)',
    wordBreak: 'break-all',
    userSelect: 'all',
    lineHeight: '1.4',
  },
  copyBtn: {
    flexShrink: 0,
    opacity: {
      default: 0.5,
      ':hover': 1,
    },
    transition: 'opacity 0.15s ease',
  },
  footer: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'flex-end',
    gap: '8px',
    animation: `${fadeIn} 0.4s ease-out`,
  },
});

/**
 * Show a modal prompting the user to open the worktree directory in their IDE.
 * Only intended for browser platform (VSCode has direct vscode.openFolder integration).
 */
export function showWorktreeOpenInIDEModal(worktreePath: string, worktreeName?: string): void {
  const displayName = worktreeName ?? worktreePath.split(/[/\\]/).pop() ?? worktreePath;

  showModal<void>({
    type: 'custom',
    title: `Open "${displayName}" in IDE`,
    icon: 'folder-opened',
    component: ({returnResultAndDismiss}) => (
      <WorktreeIDEModalContent
        worktreePath={worktreePath}
        onDismiss={() => returnResultAndDismiss(undefined)}
      />
    ),
  });
}

function WorktreeIDEModalContent({
  worktreePath,
  onDismiss,
}: {
  worktreePath: string;
  onDismiss: () => void;
}) {
  const command = `code ${worktreePath}`;

  const handleCopy = () => {
    copyAndShowToast(command);
    setTimeout(onDismiss, 400);
  };

  return (
    <div {...stylex.props(styles.container)}>
      <div {...stylex.props(styles.hint)}>
        <T>ISL switched to this worktree. Paste this in your terminal to open it:</T>
      </div>

      <div {...stylex.props(styles.commandBlock)}>
        <div {...stylex.props(styles.commandHeader)}>
          <div {...stylex.props(styles.terminalDot)} />
          Terminal
        </div>
        <div {...stylex.props(styles.commandRow)}>
          <span {...stylex.props(styles.prompt)}>$</span>
          <span {...stylex.props(styles.commandText)}>{command}</span>
          <Button icon {...stylex.props(styles.copyBtn)} onClick={handleCopy}>
            <Icon icon="copy" />
          </Button>
        </div>
      </div>

      <div {...stylex.props(styles.footer)}>
        <Button onClick={onDismiss}>
          <T>Dismiss</T>
        </Button>
      </div>
    </div>
  );
}
