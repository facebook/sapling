/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {render, screen} from '@testing-library/react';
import {ThemedComponentsRoot} from '../ThemedComponentsRoot';
import {ViewportOverlayRoot} from '../ViewportOverlay';
import ComponentExplorer from '../explorer/ComponentExplorer';

describe('component library', () => {
  it('renders component explorer', () => {
    render(
      <ThemedComponentsRoot theme="light">
        <ComponentExplorer />
        <ViewportOverlayRoot />
      </ThemedComponentsRoot>,
    );

    // the whole page appears
    expect(screen.getByText('Component Explorer')).toBeInTheDocument();
    // some buttons appear
    expect(screen.getAllByText('Primary').length).toBeGreaterThan(0);
  });
});
