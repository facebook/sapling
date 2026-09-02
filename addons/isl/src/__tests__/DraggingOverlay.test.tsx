/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DragHandler} from '../DragHandle';

import {render, screen} from '@testing-library/react';
import {ViewportOverlayRoot} from 'isl-components/ViewportOverlay';
import {DraggingOverlay} from '../DraggingOverlay';

describe('DraggingOverlay', () => {
  beforeEach(() => {
    // The overlay portals into the ViewportOverlayRoot, which registers its
    // container in an effect — render it first so the container exists.
    render(<ViewportOverlayRoot />);
  });

  it('registers the drag handler, moves the overlay, and clears the ref on unmount', () => {
    const onDragRef: React.MutableRefObject<DragHandler | null> = {current: null};
    const {unmount} = render(<DraggingOverlay onDragRef={onDragRef}>drag content</DraggingOverlay>);

    expect(onDragRef.current).toBeInstanceOf(Function);

    const draggingDiv = screen.getByText('drag content').parentElement as HTMLElement;
    onDragRef.current?.(10, 20, true);
    expect(draggingDiv.style.opacity).toBe('1');
    expect(draggingDiv.style.transform).toContain('translate(calc(10px');
    onDragRef.current?.(10, 20, false);
    expect(draggingDiv.style.opacity).toBe('0');

    unmount();
    expect(onDragRef.current).toBeNull();
  });

  it('unmount cleanup does not clear a handler installed by a newer overlay', () => {
    const onDragRef: React.MutableRefObject<DragHandler | null> = {current: null};
    const first = render(<DraggingOverlay onDragRef={onDragRef}>first</DraggingOverlay>);
    const firstHandler = onDragRef.current;

    render(<DraggingOverlay onDragRef={onDragRef}>second</DraggingOverlay>);
    const secondHandler = onDragRef.current;
    expect(secondHandler).not.toBe(firstHandler);

    first.unmount();
    expect(onDragRef.current).toBe(secondHandler);
  });
});
