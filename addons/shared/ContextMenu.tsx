/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {findParentWithClassName} from './utils';
import {useEffect, useRef, useState} from 'react';
import {atom, useRecoilState, useSetRecoilState} from 'recoil';

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
  const setState = useSetRecoilState(contextMenuState);
  return e => {
    setState({x: e.clientX, y: e.clientY, items: creator()});

    e.preventDefault();
    e.stopPropagation();
  };
}

type ContextMenuData = {x: number; y: number; items: Array<ContextMenuItem>};
type ContextMenuItem =
  | {type?: undefined; label: string | React.ReactNode; onClick?: () => void}
  | {type: 'divider'};

const contextMenuState = atom<null | ContextMenuData>({
  key: 'contextMenuState',
  default: null,
});

export function ContextMenus() {
  const [state, setState] = useRecoilState(contextMenuState);

  // after you click on an item, flash it as selected, then fade out tooltip
  const [acceptedSuggestion, setAcceptedSuggestion] = useState<null | number>(null);

  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (state != null) {
      const hide = (e: Event) => {
        if (e.type === 'keyup') {
          if ((e as KeyboardEvent).key === 'Escape') {
            setState(null);
          }
          return;
        } else if (e.type === 'click') {
          // if the click is inside the context menu, don't dismiss
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

  const topOrBottom = state.y > window.innerHeight / 2 ? 'bottom' : 'top';
  const leftOrRight = state.x > window.innerWidth / 2 ? 'right' : 'left';
  const yOffset = 10;
  const xOffset = -5;
  let position;
  if (topOrBottom === 'top') {
    if (leftOrRight === 'left') {
      position = {top: state.y + yOffset, left: state.x + xOffset};
    } else {
      position = {top: state.y + yOffset, right: window.innerWidth - (state.x + xOffset)};
    }
  } else {
    if (leftOrRight === 'left') {
      position = {bottom: window.innerHeight - (state.y - yOffset), left: state.x + xOffset};
    } else {
      position = {
        bottom: window.innerHeight - (state.y - yOffset),
        right: window.innerWidth - (state.x + xOffset),
      };
    }
  }

  return (
    <div
      ref={ref}
      className={
        'context-menu-container' + (acceptedSuggestion != null ? ' context-menu-fadeout' : '')
      }
      style={position}>
      {topOrBottom === 'top' ? (
        <div className={`context-menu-arrow-top context-menu-arrow-${leftOrRight}`} />
      ) : null}
      <div className="context-menu">
        {state.items.map((item, i) =>
          item.type === 'divider' ? (
            <div className="context-menu-divider" key={i} />
          ) : (
            <div
              key={i}
              onClick={
                // don't allow double-clicking to run the action twice
                acceptedSuggestion != null
                  ? undefined
                  : () => {
                      item.onClick?.();
                      setAcceptedSuggestion(i);
                      setTimeout(() => {
                        setState(null);
                        setAcceptedSuggestion(null);
                      }, 300);
                    }
              }
              className={
                'context-menu-item' +
                (acceptedSuggestion != null && acceptedSuggestion === i
                  ? ' context-menu-item-selected'
                  : '')
              }>
              {item.label}
            </div>
          ),
        )}
      </div>

      {topOrBottom === 'bottom' ? (
        <div className={`context-menu-arrow-bottom context-menu-arrow-${leftOrRight}`} />
      ) : null}
    </div>
  );
}
