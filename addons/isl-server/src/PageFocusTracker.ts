/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PageVisibility} from 'isl/src/types';

/**
 * Aggregates if any ISL page has focus or visibility.
 */
export class PageFocusTracker {
  private focusedPages = new Set();
  private visiblePages = new Set();

  private onChangeHandlers = new Set<(state: PageVisibility) => unknown>();

  setState(page: string, state: PageVisibility) {
    switch (state) {
      case 'focused':
        this.focusedPages.add(page);
        this.visiblePages.add(page);
        break;
      case 'visible':
        this.focusedPages.delete(page);
        this.visiblePages.add(page);
        break;
      case 'hidden':
        this.focusedPages.delete(page);
        this.visiblePages.delete(page);
        break;
    }
    for (const handler of this.onChangeHandlers) {
      handler(state);
    }
  }

  public disposePage(page: string) {
    this.focusedPages.delete(page);
    this.visiblePages.delete(page);
  }

  public hasPageWithFocus() {
    return this.focusedPages.size > 0;
  }
  public hasVisiblePage() {
    return this.visiblePages.size > 0;
  }

  public onChange(callback: () => unknown): () => void {
    this.onChangeHandlers.add(callback);
    return () => this.onChangeHandlers.delete(callback);
  }
}
