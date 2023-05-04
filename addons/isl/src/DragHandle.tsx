/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PointerEventHandler, ReactElement} from 'react';

import {Icon} from 'shared/Icon';

export type DragHandler = (x: number, y: number, isDragging: boolean) => void;

/**
 * A drag handle that fires events on drag-n-drop.
 *
 * At the start of dragging, or during dragging, call `onDrag(x, y, true)`.
 * At the end of dragging, call `onDrag(x, y, false)`.
 * `x`, `y` are relative to viewport, comparable to `getBoundingClientRect()`.
 *
 * This component renders children or the "gripper" icon to grab and updates
 * the cursor style. It does not draw the element being dragged during
 * dragging. The callstie might use a `position: fixed; left: 0; top: 0`
 * element and move it using `transform: translate(x,y)` during dragging.
 */
export function DragHandle(props: {onDrag?: DragHandler; children?: ReactElement}): ReactElement {
  const handlePointerDown: PointerEventHandler = e => {
    if (e.isPrimary) {
      const handlePointerMove = (e: PointerEvent) => {
        const target = e.target as HTMLBodyElement;
        if (target.hasPointerCapture(e.pointerId)) {
          props.onDrag?.(e.clientX, e.clientY, true);
        }
      };
      const handlePointerUp = (e: PointerEvent) => {
        const target = e.target as HTMLBodyElement;
        if (target.hasPointerCapture(e.pointerId)) {
          target.releasePointerCapture(e.pointerId);
          target.removeEventListener('pointermove', handlePointerMove as EventListener);
          target.removeEventListener('pointerup', handlePointerUp as EventListener);
          target.style.removeProperty('cursor');
          props.onDrag?.(e.clientX, e.clientY, false);
        }
      };

      // e.target might be unmounted and lose events, listen on `document.body` instead.
      const body = (e.target as HTMLSpanElement).ownerDocument.body;

      body.addEventListener('pointermove', handlePointerMove);
      body.addEventListener('pointerup', handlePointerUp);
      body.setPointerCapture(e.pointerId);
      body.style.cursor = 'grabbing';

      props.onDrag?.(e.clientX, e.clientY, true);
    }
  };

  return (
    <span className="drag-handle" onPointerDown={handlePointerDown}>
      {props.children ?? <Icon icon="gripper" />}
    </span>
  );
}
