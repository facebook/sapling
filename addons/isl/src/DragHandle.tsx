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
  return (
    <span className="drag-handle" {...dragHandleProps(props.onDrag)}>
      {props.children ?? <Icon icon="gripper" />}
    </span>
  );
}

/**
 * Return React properties to handle customized dragging.
 *
 * At the start of dragging, or during dragging, call `onDrag(x, y, true)`.
 * At the end of dragging, call `onDrag(x, y, false)`.
 * `x`, `y` are relative to viewport, comparable to `getBoundingClientRect()`.
 */
export function dragHandleProps(onDrag?: DragHandler): {
  onDragStart?: React.DragEventHandler<unknown>;
  onPointerDown?: PointerEventHandler<unknown>;
} {
  if (onDrag == null) {
    return {};
  }
  let pointerDown = false;
  const handlePointerDown: PointerEventHandler = e => {
    if (e.isPrimary && !pointerDown) {
      // e.target might be unmounted and lose events, listen on `document.body` instead.
      const body = (e.target as HTMLSpanElement).ownerDocument.body;

      const handlePointerMove = (e: PointerEvent) => {
        onDrag(e.clientX, e.clientY, true);
      };
      const handlePointerUp = (e: PointerEvent) => {
        body.removeEventListener('pointermove', handlePointerMove as EventListener);
        body.removeEventListener('pointerup', handlePointerUp as EventListener);
        body.removeEventListener('pointerleave', handlePointerUp as EventListener);
        body.releasePointerCapture(e.pointerId);
        body.style.removeProperty('cursor');
        pointerDown = false;
        onDrag(e.clientX, e.clientY, false);
      };

      body.setPointerCapture(e.pointerId);
      body.addEventListener('pointermove', handlePointerMove);
      body.addEventListener('pointerup', handlePointerUp);
      body.addEventListener('pointerleave', handlePointerUp);

      body.style.cursor = 'grabbing';
      pointerDown = true;

      onDrag(e.clientX, e.clientY, true);
    }
  };

  return {
    onDragStart: e => e.preventDefault(),
    onPointerDown: handlePointerDown,
  };
}
