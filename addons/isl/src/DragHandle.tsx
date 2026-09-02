/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PointerEventHandler, ReactElement} from 'react';

import {Icon} from 'isl-components/Icon';
import {cn} from 'shared/cn';

export type DragHandler = (x: number, y: number, isDragging: boolean) => void;

type DragPosition = {x: number; y: number};

class PointerDragSession {
  private animationFrame: number | null = null;
  private ended = false;
  private pendingPosition: DragPosition | null = null;

  constructor(
    private body: HTMLElement,
    private view: Window | null,
    private pointerId: number,
    private onDrag: DragHandler,
    private onEnd: () => void,
  ) {
    body.setPointerCapture(pointerId);
    body.addEventListener('pointermove', this.handlePointerMove);
    body.addEventListener('pointerup', this.handlePointerUp);
    body.addEventListener('pointercancel', this.handlePointerUp);
    body.addEventListener('pointerleave', this.handlePointerUp);
    body.style.cursor = 'grabbing';
  }

  start(x: number, y: number) {
    this.onDrag(x, y, true);
  }

  private handlePointerMove = (event: PointerEvent) => {
    if (event.pointerId !== this.pointerId) {
      return;
    }
    this.pendingPosition = {x: event.clientX, y: event.clientY};
    if (this.animationFrame != null) {
      return;
    }
    if (this.view == null) {
      this.flushPendingPosition();
      return;
    }
    this.animationFrame = this.view.requestAnimationFrame(() => {
      this.animationFrame = null;
      this.flushPendingPosition();
    });
  };

  private handlePointerUp = (event: PointerEvent) => {
    if (event.pointerId !== this.pointerId) {
      return;
    }
    // pointercancel coordinates are unreliable (some engines report 0,0);
    // drop the pending move rather than applying a bogus position.
    const hasPendingPosition = this.pendingPosition != null && event.type !== 'pointercancel';
    this.dispose();
    if (hasPendingPosition) {
      this.onDrag(event.clientX, event.clientY, true);
    }
    this.onDrag(event.clientX, event.clientY, false);
  };

  private flushPendingPosition() {
    const position = this.pendingPosition;
    this.pendingPosition = null;
    if (!this.ended && position != null) {
      this.onDrag(position.x, position.y, true);
    }
  }

  private dispose() {
    if (this.ended) {
      return;
    }
    this.ended = true;
    if (this.animationFrame != null && this.view != null) {
      this.view.cancelAnimationFrame(this.animationFrame);
    }
    this.animationFrame = null;
    this.pendingPosition = null;
    this.body.removeEventListener('pointermove', this.handlePointerMove);
    this.body.removeEventListener('pointerup', this.handlePointerUp);
    this.body.removeEventListener('pointercancel', this.handlePointerUp);
    this.body.removeEventListener('pointerleave', this.handlePointerUp);
    if (this.body.hasPointerCapture?.(this.pointerId) !== false) {
      this.body.releasePointerCapture(this.pointerId);
    }
    this.body.style.removeProperty('cursor');
    this.onEnd();
  }
}

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
export function DragHandle(props: {
  onDrag?: DragHandler;
  children?: ReactElement;
  className?: string;
}): ReactElement {
  return (
    <span {...dragHandleProps(props.onDrag)} className={cn(props.className, 'drag-handle')}>
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
  let activeSession: PointerDragSession | null = null;
  const handlePointerDown: PointerEventHandler = e => {
    if (e.isPrimary && activeSession == null) {
      // e.target might be unmounted and lose events, listen on `document.body` instead.
      const body = (e.target as HTMLSpanElement).ownerDocument.body;
      const view = body.ownerDocument.defaultView;
      let session: PointerDragSession;
      session = new PointerDragSession(body, view, e.pointerId, onDrag, () => {
        if (activeSession === session) {
          activeSession = null;
        }
      });
      activeSession = session;
      session.start(e.clientX, e.clientY);
    }
  };

  return {
    onDragStart: e => e.preventDefault(),
    onPointerDown: handlePointerDown,
  };
}
