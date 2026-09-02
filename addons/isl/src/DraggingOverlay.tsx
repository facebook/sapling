/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DragHandler} from './DragHandle';

import {ViewportOverlay} from 'isl-components/ViewportOverlay';
import {getZoomLevel} from 'isl-components/zoom';
import React, {useLayoutEffect, useRef} from 'react';
import css from './DraggingOverlay.module.css';

type DraggingOverlayProps = React.HTMLProps<HTMLDivElement> & {
  /**
   * Callback ref to update the position of the element.
   *
   * It is compatible with the `onDrag: DragHandler` property of `DragHandler`,
   * or the `clientX`, `clientY` properties of the 'pointermove' event on
   * `document.body`.
   */
  onDragRef: React.MutableRefObject<DragHandler | null>;

  /** X offset. Default: `- var(--pad)`. */
  dx?: string;

  /** Y offset. Default: `- 50%`. */
  dy?: string;

  /** Extra "hint" message. Will be rendered as a tooltip. */
  hint?: string | null;
};

/**
 * Render children as the "dragging overlay".
 *
 * The callsite needs to update the content (children) and position of
 * the dragging overlay. For performance, the position update requires
 * the callsite to call `props.onDragRef.current` instead of using React
 * props.
 */
export function DraggingOverlay(props: DraggingOverlayProps) {
  const draggingDivRef = useRef<HTMLDivElement | null>(null);
  const {key, children, onDragRef, dx = '- var(--pad)', dy = '- 50%', hint, ...rest} = props;

  useLayoutEffect(() => {
    const handleDrag: DragHandler = (x, y, isDragging) => {
      const draggingDiv = draggingDivRef.current;
      if (draggingDiv != null) {
        if (isDragging) {
          const zoom = getZoomLevel();
          Object.assign(draggingDiv.style, {
            transform: `translate(calc(${Math.round(x / zoom)}px ${dx}), calc(${Math.round(
              y / zoom,
            )}px ${dy}))`,
            opacity: '1',
          });
        } else {
          draggingDiv.style.opacity = '0';
        }
      }
    };
    onDragRef.current = handleDrag;
    return () => {
      if (onDragRef.current === handleDrag) {
        onDragRef.current = null;
      }
    };
  }, [dx, dy, onDragRef]);

  return (
    <ViewportOverlay key={key}>
      <div style={{width: 'fit-content', opacity: 0}} ref={draggingDivRef}>
        <div className={css.draggingElement} {...rest}>
          {children}
        </div>
        {hint != null && (
          <div className={css.hint}>
            <span className="tooltip" style={{height: 'fit-content'}}>
              {hint}
            </span>
          </div>
        )}
      </div>
    </ViewportOverlay>
  );
}
