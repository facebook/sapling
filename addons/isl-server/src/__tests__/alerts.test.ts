/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {parseAlerts} from '../alerts';

describe('alerts', () => {
  it('parses valid alerts', () => {
    expect(
      parseAlerts([
        {name: 'alerts.S12345.title', value: 'Rebases broken'},
        {name: 'alerts.S12345.description', value: 'Fix is being deployed'},
        {name: 'alerts.S12345.severity', value: 'SEV 1'},
        {name: 'alerts.S12345.url', value: 'https://sapling-scm.com'},
        {name: 'alerts.S12345.show-in-isl', value: 'true'},
        {name: 'alerts.S12345.isl-version-regex', value: '0.1.38.*'},
      ]),
    ).toEqual([
      {
        key: 'S12345',
        title: 'Rebases broken',
        description: 'Fix is being deployed',
        severity: 'SEV 1',
        url: 'https://sapling-scm.com',
        ['show-in-isl']: true,
        ['isl-version-regex']: '0.1.38.*',
      },
    ]);
  });

  it('can parse multiple alerts', () => {
    expect(
      parseAlerts([
        {name: 'alerts.S11111.title', value: 'Rebases broken'},
        {name: 'alerts.S22222.title', value: 'Goto broken'},
        {name: 'alerts.S11111.description', value: 'Fix is being deployed'},
        {name: 'alerts.S22222.description', value: 'Fix is being deployed'},
        {name: 'alerts.S11111.severity', value: 'SEV 1'},
        {name: 'alerts.S22222.severity', value: 'SEV 2'},
        {name: 'alerts.S11111.url', value: 'https://sapling-scm.com'},
        {name: 'alerts.S22222.url', value: 'https://sapling-scm.com'},
        {name: 'alerts.S11111.show-in-isl', value: 'true'},
        {name: 'alerts.S22222.show-in-isl', value: 'true'},
      ]),
    ).toEqual([
      {
        key: 'S11111',
        title: 'Rebases broken',
        description: 'Fix is being deployed',
        severity: 'SEV 1',
        url: 'https://sapling-scm.com',
        ['show-in-isl']: true,
      },
      {
        key: 'S22222',
        title: 'Goto broken',
        description: 'Fix is being deployed',
        severity: 'SEV 2',
        url: 'https://sapling-scm.com',
        ['show-in-isl']: true,
      },
    ]);
  });

  it('excludes alerts not for ISL', () => {
    expect(
      parseAlerts([
        {name: 'alerts.S12345.title', value: 'Rebases broken'},
        {name: 'alerts.S12345.description', value: 'Fix is being deployed'},
        {name: 'alerts.S12345.severity', value: 'SEV 1'},
        {name: 'alerts.S12345.url', value: 'https://sapling-scm.com'},
        {name: 'alerts.S12345.show-in-isl', value: 'false'},
      ]),
    ).toEqual([]);
    expect(
      parseAlerts([
        {name: 'alerts.S12345.title', value: 'Rebases broken'},
        {name: 'alerts.S12345.description', value: 'Fix is being deployed'},
        {name: 'alerts.S12345.severity', value: 'SEV 1'},
        {name: 'alerts.S12345.url', value: 'https://sapling-scm.com'},
      ]),
    ).toEqual([]);
  });

  it('excludes alerts missing fields', () => {
    expect(
      parseAlerts([
        {name: 'alerts.S12345.title', value: 'Rebases broken'},
        {name: 'alerts.S12345.show-in-isl', value: 'true'},
      ]),
    ).toEqual([]);
    expect(parseAlerts([{name: 'alerts.S12345.show-in-isl', value: 'true'}])).toEqual([]);
  });
});
