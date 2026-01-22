/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ComponentClass} from 'react';
import type {EnsureAssignedTogether} from 'shared/EnsureAssignedTogether';

import {useAtom} from 'jotai';
import {createElement, useCallback, useRef, useState} from 'react';
import {debounce} from 'shared/debounce';
import {islDrawerState} from './drawerState';

import './Drawers.css';

type NonNullReactElement = React.ReactElement | React.ReactFragment;

enum Side {
  left = 'left',
  right = 'right',
  top = 'top',
  bottom = 'bottom',
}

export type AllDrawersState = {[s in Side]: DrawerState};
export type DrawerState = {size: number; collapsed: boolean};

export type ErrorBoundaryComponent = ComponentClass<
  {children: React.ReactNode},
  {error: Error | null}
>;

export function Drawers({
  right,
  rightLabel,
  left,
  leftLabel,
  top,
  topLabel,
  bottom,
  bottomLabel,
  errorBoundary,
  children,
}: {
  errorBoundary: ErrorBoundaryComponent;
  children: React.ReactNode;
} & EnsureAssignedTogether<{left: NonNullReactElement; leftLabel: NonNullReactElement}> &
  EnsureAssignedTogether<{right: NonNullReactElement; rightLabel: NonNullReactElement}> &
  EnsureAssignedTogether<{top: NonNullReactElement; topLabel: NonNullReactElement}> &
  EnsureAssignedTogether<{bottom: NonNullReactElement; bottomLabel: NonNullReactElement}>) {
  return (
    <div className="drawers">
      {top ? (
        <Drawer side={Side.top} label={topLabel} errorBoundary={errorBoundary}>
          {top}
        </Drawer>
      ) : null}
      <div className="drawers-horizontal">
        {left ? (
          <Drawer side={Side.left} label={leftLabel} errorBoundary={errorBoundary}>
            {left}
          </Drawer>
        ) : null}
        <div className="drawer-main-content">{children}</div>
        {right ? (
          <Drawer side={Side.right} label={rightLabel} errorBoundary={errorBoundary}>
            {right}
          </Drawer>
        ) : null}
      </div>

      {bottom ? (
        <Drawer side={Side.bottom} label={bottomLabel} errorBoundary={errorBoundary}>
          {bottom}
        </Drawer>
      ) : null}
    </div>
  );
}

const stickyCollapseSizePx = 60;
const minDrawerSizePx = 100;

export function Drawer({
  side,
  label,
  errorBoundary,
  children,
}: {
  side: Side;
  label: React.ReactNode;
  errorBoundary: ErrorBoundaryComponent;
  children: NonNullReactElement;
}) {
  const isVertical = side === 'top' || side === 'bottom';
  const dragHandleElement = useRef<HTMLDivElement>(null);
  const [isResizing, setIsResizing] = useState(false);

  const [drawerState, setDrawerState] = useAtom(islDrawerState);
  const state = drawerState[side];
  const isExpanded = !state.collapsed;

  const setInnerState = useCallback(
    (callback: (prevState: DrawerState) => DrawerState) =>
      setDrawerState(prev => ({...prev, [side]: callback(prev[side])})),
    [side, setDrawerState],
  );
  const startResizing = useCallback(
    (e: React.MouseEvent, initialWidth: number) => {
      e.preventDefault();
      const start = isVertical ? e.clientY : e.clientX;
      setIsResizing(true);

      const moveHandler = debounce(
        (newE: MouseEvent) => {
          const newPos = isVertical ? newE.clientY : newE.clientX;
          const maxDrawerSizePx = isVertical ? window.innerHeight : window.innerWidth;
          const newSize =
            side === 'right' || side === 'bottom'
              ? initialWidth - (newPos - start)
              : initialWidth + (newPos - start);
          setInnerState((_prev: DrawerState) => ({
            size: Math.min(maxDrawerSizePx, newSize),
            // if resizing would give us a very small size, just collapse the view entirely
            // note we don't stop the drag sequence by doing this, you can just drag back a bit to re-expand
            collapsed: newSize > stickyCollapseSizePx ? false : true,
          }));
        },
        2,
        undefined,
        true,
      );
      window.addEventListener('mousemove', moveHandler);

      const onMouseUp = () => {
        setIsResizing(false);
        dispose?.();
        dispose = undefined;
      };

      let dispose: (() => void) | undefined = () => {
        window.removeEventListener('mousemove', moveHandler);
        window.removeEventListener('mouseup', onMouseUp);
      };

      window.addEventListener('mouseup', onMouseUp);
      return dispose;
    },
    [isVertical, side, setInnerState],
  );

  return (
    <div
      className={`drawer drawer-${side}${isExpanded ? ' drawer-expanded' : ''}`}
      style={isExpanded ? {[isVertical ? 'height' : 'width']: `${state.size}px`} : undefined}>
      <div
        className="drawer-label"
        data-testid="drawer-label"
        onClick={() => {
          const maxDrawerSizePx = isVertical ? window.innerHeight : window.innerWidth;
          setDrawerState(prev => ({
            ...prev,
            [side]: {
              // enforce min/max size when expanding
              size: Math.min(maxDrawerSizePx, Math.max(minDrawerSizePx, prev[side].size)),
              collapsed: !prev[side].collapsed,
            },
          }));
        }}>
        {label}
      </div>
      {isExpanded ? (
        <>
          <div
            ref={dragHandleElement}
            className={`resizable-drag-handle${isResizing ? ' resizing' : ''}`}
            onMouseDown={(e: React.MouseEvent) => startResizing(e, state.size)}
          />
          {createElement(errorBoundary, null, children)}
        </>
      ) : null}
    </div>
  );
}
