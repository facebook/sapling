/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {HTMLAttributes, ReactNode} from 'react';

import './InlineBadge.css';

export function InlineBadge({
  children,
  kind,
  onClick,
  onKeyDown,
  role,
  tabIndex,
  ...rest
}: {
  children: ReactNode;
  kind?: 'primary' | 'secondary' | 'attention';
} & HTMLAttributes<HTMLDivElement>) {
  const restWithContextMenu = rest as HTMLAttributes<HTMLDivElement>;
  const isInteractive = onClick != null || restWithContextMenu.onContextMenu != null;
  const hasMenu = restWithContextMenu.onContextMenu != null;
  // Only announce a popup when *activating* (click/Enter/Space) the badge opens it.
  // If a context menu is only reachable via right-click/Shift+F10 while onClick does
  // something else (e.g. navigation), advertising `aria-haspopup` would mislead
  // screen reader users about what the primary action does. When there's no onClick
  // at all, activation falls back to opening the menu (see handleKeyDown below).
  const clickOpensMenu =
    hasMenu && (onClick == null || onClick === restWithContextMenu.onContextMenu);
  // Keyboard events have no clientX/Y (they would be NaN), so synthesize a
  // MouseEvent using the target's bounding-rect center as coordinates. Used for
  // both context-menu and plain onClick invocations so consumers reading
  // clientX/clientY always receive real values.
  const synthesizeMouseFromKeyboard = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return {
      ...e,
      clientX: rect.left + rect.width / 2,
      clientY: rect.top + rect.height / 2,
      preventDefault: () => e.preventDefault(),
      stopPropagation: () => e.stopPropagation(),
    } as unknown as React.MouseEvent<HTMLDivElement>;
  };
  const triggerContextMenuFromKeyboard = (e: React.KeyboardEvent<HTMLDivElement>) => {
    (restWithContextMenu.onContextMenu as unknown as ((ev: unknown) => void) | undefined)?.(
      synthesizeMouseFromKeyboard(e),
    );
  };
  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (isInteractive && (e.key === 'Enter' || e.key === ' ')) {
      e.preventDefault();
      if (onClick != null) {
        // If onClick is the menu handler (VS Code: onClick===onContextMenu),
        // it reads clientX/Y — synthesize centered coords instead of raw keyboard event.
        if (clickOpensMenu) {
          triggerContextMenuFromKeyboard(e);
        } else {
          (onClick as unknown as ((ev: unknown) => void) | undefined)?.(
            synthesizeMouseFromKeyboard(e),
          );
        }
      } else if (hasMenu) {
        triggerContextMenuFromKeyboard(e);
      }
    } else if (
      isInteractive &&
      hasMenu &&
      (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10'))
    ) {
      e.preventDefault();
      triggerContextMenuFromKeyboard(e);
    }
    // Preserve any explicit onKeyDown from caller
    (onKeyDown as unknown as ((ev: unknown) => void) | undefined)?.(e);
  };
  return (
    <div
      {...rest}
      className={`inline-badge badge-${kind ?? 'secondary'}`}
      role={isInteractive ? (role ?? 'button') : role}
      tabIndex={isInteractive ? (tabIndex ?? 0) : tabIndex}
      aria-haspopup={clickOpensMenu ? 'menu' : undefined}
      onClick={onClick}
      onKeyDown={handleKeyDown}>
      {children}
    </div>
  );
}
