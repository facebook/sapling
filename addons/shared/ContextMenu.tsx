/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from 'isl-components/Icon';
import {findParentWithClassName} from 'isl-components/utils';
import {getZoomLevel} from 'isl-components/zoom';
import {atom, useAtom, useSetAtom} from 'jotai';
import React, {useEffect, useRef, useState} from 'react';

import './ContextMenu.css';

/**
 * Hook to create a context menu in HTML.
 * Pass in a function that returns the list of context menu items.
 * Then use the result in onContextMenu:
 * ```
 * function MyComponent() {
 *   const menu = useContextMenu(() => [
 *     {label: 'Choice 1', onClick: () => console.log('clicked!')}
 *   ]);
 *   return <div onContextMenu={menu}>...</div>
 * }
 * ```
 */
export function useContextMenu<T>(
  creator: () => Array<ContextMenuItem>,
): React.MouseEventHandler<T> {
  const setState = useSetAtom(contextMenuState);
  return e => {
    const zoom = getZoomLevel();
    const items = creator();
    if (items.length === 0) {
      return;
    }
    setState({x: e.clientX / zoom, y: e.clientY / zoom, items});

    e.preventDefault();
    e.stopPropagation();
  };
}

type ContextMenuData = {x: number; y: number; items: Array<ContextMenuItem>};
export type ContextMenuItem =
  | {
      type?: undefined;
      label: string | React.ReactNode;
      onClick?: (e?: MouseEvent) => void;
      tooltip?: string | React.ReactNode;
    }
  | {
      type: 'submenu';
      label: string | React.ReactNode;
      children: Array<ContextMenuItem>;
    }
  | {type: 'divider'};

export const contextMenuState = atom<null | ContextMenuData>(null);

/**
 * Compute the absolute placement for the context menu overlay from the click
 * point, the window size, and the zoom level. The menu anchors to whichever
 * viewport quadrant the click is in and grows toward the center; the `maxHeight`
 * / `maxWidth` clamps keep it inside the viewport so it never spills off an edge
 * (e.g. off the left of a narrow pane). Exported for unit testing.
 */
export function computeContextMenuStyle(
  state: {x: number; y: number},
  windowSize: {innerWidth: number; innerHeight: number},
  zoom: number,
): {
  position: React.CSSProperties;
  topOrBottom: 'top' | 'bottom';
  leftOrRight: 'left' | 'right';
} {
  const topOrBottom = state.y > windowSize.innerHeight / zoom / 2 ? 'bottom' : 'top';
  const leftOrRight = state.x > windowSize.innerWidth / zoom / 2 ? 'right' : 'left';
  const yOffset = 10;
  const xOffset = -10; // var(--pad)
  let position: React.CSSProperties;
  if (topOrBottom === 'top') {
    if (leftOrRight === 'left') {
      position = {top: state.y + yOffset, left: state.x + xOffset};
    } else {
      position = {
        top: state.y + yOffset,
        right: windowSize.innerWidth / zoom - (state.x - xOffset),
      };
    }
  } else {
    if (leftOrRight === 'left') {
      position = {
        bottom: windowSize.innerHeight / zoom - (state.y - yOffset),
        left: state.x + xOffset,
      };
    } else {
      position = {
        bottom: windowSize.innerHeight / zoom - (state.y - yOffset),
        right: windowSize.innerWidth / zoom - (state.x - xOffset),
      };
    }
  }
  position.maxHeight =
    windowSize.innerHeight / zoom -
    ((position.top as number | null) ?? 0) -
    ((position.bottom as number | null) ?? 0);
  // Clamp width the same way as maxHeight so a menu anchored near one edge cannot
  // grow off the opposite edge in a narrow pane; keep the 500px cap (matching
  // .context-menu-container in ContextMenu.css) when the viewport has room.
  position.maxWidth = Math.min(
    500,
    windowSize.innerWidth / zoom -
      ((position.left as number | null) ?? 0) -
      ((position.right as number | null) ?? 0),
  );
  return {position, topOrBottom, leftOrRight};
}

export function ContextMenus() {
  const [state, setState] = useAtom(contextMenuState);

  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (state != null) {
      const hide = (e: Event) => {
        if (e.type === 'keyup') {
          if ((e as KeyboardEvent).key === 'Escape') {
            setState(null);
          }
          return;
        } else if (e.type === 'click' || e.type === 'scroll') {
          // if click or scroll inside the context menu, don't dismiss
          if (findParentWithClassName(e.target as HTMLElement, 'context-menu-container')) {
            return;
          }
        }
        setState(null);
      };
      window.addEventListener('click', hide, true);
      window.addEventListener('scroll', hide, true);
      window.addEventListener('resize', hide, true);
      window.addEventListener('keyup', hide, true);
      return () => {
        window.removeEventListener('click', hide, true);
        window.removeEventListener('scroll', hide, true);
        window.removeEventListener('resize', hide, true);
        window.removeEventListener('keyup', hide, true);
      };
    }
  }, [state, setState]);

  if (state == null) {
    return null;
  }

  const zoom = getZoomLevel();
  const {position, topOrBottom, leftOrRight} = computeContextMenuStyle(state, window, zoom);

  return (
    <div
      ref={ref}
      className={'context-menu-container'}
      data-testid="context-menu-container"
      style={position}>
      {topOrBottom === 'top' ? (
        <div
          className={`context-menu-arrow context-menu-arrow-top context-menu-arrow-${leftOrRight}`}
        />
      ) : null}
      <ContextMenuList
        items={state.items}
        clickItem={(item, e) => {
          if (item.type != null) {
            return;
          }
          // don't allow double-clicking to run the action twice
          if (state != null) {
            item.onClick?.(e);
            setState(null);
          }
        }}
      />

      {topOrBottom === 'bottom' ? (
        <div
          className={`context-menu-arrow context-menu-arrow-bottom context-menu-arrow-${leftOrRight}`}
        />
      ) : null}
    </div>
  );
}

function ContextMenuList({
  items,
  clickItem,
}: {
  items: Array<ContextMenuItem>;
  clickItem: (item: ContextMenuItem, e?: MouseEvent) => void;
}) {
  // Each ContextMenuList renders one additional layer of submenu
  const [submenuNavigation, setSubmenuNavigation] = useState<
    {x: number; y: number; children: Array<ContextMenuItem>} | undefined
  >(undefined);
  const [tooltip, setTooltip] = useState<
    {x: number; y: number; tooltip: string | React.ReactNode} | undefined
  >(undefined);
  const ref = useRef<HTMLDivElement | null>(null);

  function getCoordinatesForSubElement(e: React.PointerEvent) {
    const target = e.currentTarget as HTMLElement;
    const parent = ref.current;
    if (!parent) {
      return;
    }
    const parentRect = parent?.getBoundingClientRect();
    const rect = target.getBoundingClientRect();
    // attach to top right corner
    const x = -1 * parentRect.left + rect.right;
    const y = -1 * parentRect.top + rect.top;
    return {x, y};
  }

  return (
    <>
      <div className="context-menu" ref={ref} onPointerLeave={() => setTooltip(undefined)}>
        {items.map((item, i) =>
          item.type === 'divider' ? (
            <div className="context-menu-divider" key={i} />
          ) : item.type === 'submenu' ? (
            <div
              key={i}
              className={'context-menu-item context-menu-submenu'}
              onPointerEnter={e => {
                const coordinates = getCoordinatesForSubElement(e);
                if (!coordinates) {
                  return;
                }
                const {x, y} = coordinates;
                setSubmenuNavigation({
                  x,
                  y,
                  children: item.children,
                });
                setTooltip(undefined);
              }}>
              <span>{item.label}</span>
              <Icon icon="chevron-right" />
            </div>
          ) : (
            <div
              key={i}
              onPointerEnter={e => {
                if (item.tooltip) {
                  const coordinates = getCoordinatesForSubElement(e);
                  if (!coordinates) {
                    return;
                  }
                  const {x, y} = coordinates;
                  setTooltip({x, y, tooltip: item.tooltip});
                } else {
                  setTooltip(undefined);
                }
                setSubmenuNavigation(undefined);
              }}
              onClick={e => {
                clickItem(item, e.nativeEvent);
              }}
              className={'context-menu-item'}>
              {item.label}
            </div>
          ),
        )}
      </div>
      {submenuNavigation != null && (
        <div
          className="context-menu-submenu-navigation"
          style={{position: 'absolute', top: submenuNavigation.y, left: submenuNavigation.x}}>
          <ContextMenuList items={submenuNavigation.children} clickItem={clickItem} />
        </div>
      )}
      {tooltip != null && (
        <div
          className="context-menu-tooltip"
          style={{position: 'absolute', top: tooltip.y, left: tooltip.x}}>
          {tooltip.tooltip}
        </div>
      )}
    </>
  );
}
