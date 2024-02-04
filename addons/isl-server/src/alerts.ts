/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Alert} from 'isl/src/types';

/** Given raw json output from `sl config`, parse Alerts */
export function parseAlerts(rawConfigs: Array<{name: string; value: unknown}>): Array<Alert> {
  // we get back configs with their keys as prefixes
  // [ {name: "alerts.S11111.title", value: "alert 1"}, {name: "alerts.S22222.title", value: "alert 2"}, ...]
  const alertMap = new Map<string, Partial<Alert>>();
  for (const entry of Object.values(rawConfigs)) {
    const {name, value} = entry;
    const [, key, suffix] = name.split('.');
    if (!suffix) {
      continue;
    }
    const existing = alertMap.get(key) ?? {key};
    (existing as {[key: string]: unknown})[suffix] =
      suffix === 'show-in-isl' ? value == 'true' : value;
    alertMap.set(key, existing);
  }

  return [...alertMap.values()].filter(
    (entry): entry is Alert =>
      !!entry.title &&
      !!entry.description &&
      !!entry.severity &&
      !!entry.url &&
      entry['show-in-isl'] === true,
  );
}
