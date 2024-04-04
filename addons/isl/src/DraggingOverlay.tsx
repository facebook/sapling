/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DragHandler} from './DragHandle';

import {ViewportOverlay} from './ViewportOverlay';
import React, {useEffect, useRef} from 'react';
import {getZoomLevel} from 'shared/zoom';

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
  const {key, children, onDragRef, style, dx = '- var(--pad)', dy = '- 50%', ...rest} = props;
  const newStyle = {...style, opacity: 0};

  useEffect(() => {
    const zoom = getZoomLevel();
    onDragRef.current = (x, y, isDragging) => {
      const draggingDiv = draggingDivRef.current;
      if (draggingDiv != null) {
        if (isDragging) {
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
  });

  return (
    <ViewportOverlay key={key}>
      <div {...rest} style={newStyle} ref={draggingDivRef}>
        {children}
      </div>
    </ViewportOverlay>
  );
}
