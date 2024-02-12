/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {EnsureAssignedTogether} from './EnsureAssignedTogether';
import type {ComponentClass} from 'react';
import type {RecoilState} from 'recoil';

import {debounce} from './debounce';
import {createElement, useCallback, useRef} from 'react';
import {useRecoilState} from 'recoil';

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
  drawerState,
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
  drawerState: RecoilState<AllDrawersState>;
  errorBoundary: ErrorBoundaryComponent;
  children: React.ReactNode;
} & EnsureAssignedTogether<{left: NonNullReactElement; leftLabel: NonNullReactElement}> &
  EnsureAssignedTogether<{right: NonNullReactElement; rightLabel: NonNullReactElement}> &
  EnsureAssignedTogether<{top: NonNullReactElement; topLabel: NonNullReactElement}> &
  EnsureAssignedTogether<{bottom: NonNullReactElement; bottomLabel: NonNullReactElement}>) {
  return (
    <div className="drawers">
      {top ? (
        <Drawer
          stateAtom={drawerState}
          side={Side.top}
          label={topLabel}
          errorBoundary={errorBoundary}>
          {top}
        </Drawer>
      ) : null}
      <div className="drawers-horizontal">
        {left ? (
          <Drawer
            stateAtom={drawerState}
            side={Side.left}
            label={leftLabel}
            errorBoundary={errorBoundary}>
            {left}
          </Drawer>
        ) : null}
        <div className="drawer-main-content">{children}</div>
        {right ? (
          <Drawer
            stateAtom={drawerState}
            side={Side.right}
            label={rightLabel}
            errorBoundary={errorBoundary}>
            {right}
          </Drawer>
        ) : null}
      </div>

      {bottom ? (
        <Drawer
          stateAtom={drawerState}
          side={Side.bottom}
          label={bottomLabel}
          errorBoundary={errorBoundary}>
          {bottom}
        </Drawer>
      ) : null}
    </div>
  );
}

const stickyCollapseSizePx = 60;
const minimumDrawerSizePx = 100;

export function Drawer({
  stateAtom,
  side,
  label,
  errorBoundary,
  children,
}: {
  stateAtom: RecoilState<AllDrawersState>;
  side: Side;
  label: React.ReactNode;
  errorBoundary: ErrorBoundaryComponent;
  children: NonNullReactElement;
}) {
  const isVertical = side === 'top' || side === 'bottom';
  const dragHandleElement = useRef<HTMLDivElement>(null);

  const [drawerState, setDrawerState] = useRecoilState(stateAtom);
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

      const moveHandler = debounce(
        (newE: MouseEvent) => {
          const newPos = isVertical ? newE.clientY : newE.clientX;
          const newSize =
            side === 'right' || side === 'bottom'
              ? initialWidth - (newPos - start)
              : initialWidth + (newPos - start);
          setInnerState((_prev: DrawerState) => ({
            size: newSize,
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
          setDrawerState(prev => ({
            ...prev,
            [side]: {
              size:
                // enforce a minimum size when expanding
                prev[side].size < minimumDrawerSizePx ? minimumDrawerSizePx : prev[side].size,
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
            className="resizable-drag-handle"
            onMouseDown={(e: React.MouseEvent) => startResizing(e, state.size)}
          />
          {createElement(errorBoundary, null, children)}
        </>
      ) : null}
    </div>
  );
}
