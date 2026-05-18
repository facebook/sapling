/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {MS_PER_DAY} from 'shared/constants';
import {tryJsonParse} from 'shared/utils';

type ItemData = {count: number; lastUsed: number};

type Options = {
  /** localStorage key under which to persist this store. */
  storageKey: string;
  /** Cap on the number of items returned by getRecent(). */
  maxVisible: number;
  /**
   * Half-life for frecency decay in days. After this many days,
   * an item's recency multiplier is halved.
   */
  halfLifeDays?: number;
  /**
   * Items not used within this many days are pruned from storage on load,
   * preventing unbounded growth.
   */
  maxAgeDays?: number;
};

const DEFAULT_HALF_LIFE_DAYS = 14;
const DEFAULT_MAX_AGE_DAYS = 90;

/**
 * Frecency-based recent-items store, persisted to localStorage.
 * Combines frequency (how often used) with recency (how recently used)
 * via exponential decay. More recent usage has higher weight.
 *
 * On load, migrates the legacy on-disk format (count-only) into the
 * current `{count, lastUsed}` shape and prunes entries older than
 * `maxAgeDays`.
 */
export class FrecencyStore {
  private recent: Map<string, ItemData>;
  private readonly storageKey: string;
  private readonly maxVisible: number;
  private readonly halfLifeDays: number;
  private readonly maxAgeMs: number;

  constructor(options: Options) {
    this.storageKey = options.storageKey;
    this.maxVisible = options.maxVisible;
    this.halfLifeDays = options.halfLifeDays ?? DEFAULT_HALF_LIFE_DAYS;
    this.maxAgeMs = (options.maxAgeDays ?? DEFAULT_MAX_AGE_DAYS) * MS_PER_DAY;

    try {
      const stored = tryJsonParse(localStorage.getItem(this.storageKey) ?? '[]') as Array<
        [string, number | ItemData]
      > | null;
      this.recent = new Map();
      const now = Date.now();
      let needsPersist = false;
      if (stored) {
        for (const [key, value] of stored) {
          if (typeof value === 'number') {
            this.recent.set(key, {count: value, lastUsed: now});
            needsPersist = true;
          } else if (now - value.lastUsed <= this.maxAgeMs) {
            this.recent.set(key, value);
          } else {
            needsPersist = true;
          }
        }
      }
      if (needsPersist) {
        this.persist();
      }
    } catch {
      this.recent = new Map();
    }
  }

  private persist() {
    try {
      localStorage.setItem(this.storageKey, JSON.stringify([...this.recent.entries()]));
    } catch {}
  }

  private getFrecencyScore(data: ItemData): number {
    const daysSinceLastUse = (Date.now() - data.lastUsed) / MS_PER_DAY;
    const recencyMultiplier = Math.pow(0.5, daysSinceLastUse / this.halfLifeDays);
    return data.count * recencyMultiplier;
  }

  /** Record a usage of `item`, bumping its frecency score. */
  public use(item: string) {
    const existing = this.recent.get(item);
    this.recent.set(item, {
      count: (existing?.count ?? 0) + 1,
      lastUsed: Date.now(),
    });
    this.persist();
  }

  /** Top-N items by frecency, highest first. */
  public getRecent(): Array<string> {
    return [...this.recent.entries()]
      .map(([name, data]) => ({name, score: this.getFrecencyScore(data)}))
      .sort((a, b) => b.score - a.score)
      .slice(0, this.maxVisible)
      .map(({name}) => name);
  }
}
