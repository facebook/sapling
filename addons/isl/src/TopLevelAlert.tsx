/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {Alert, AlertSeverity} from './types';

import * as stylex from '@stylexjs/stylex';
import {Banner, BannerKind} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Subtle} from 'isl-components/Subtle';
import {atom, useAtom, useAtomValue} from 'jotai';
import {useEffect} from 'react';
import {colors, font, radius, spacing} from '../../components/theme/tokens.stylex';
import serverAPI from './ClientToServerAPI';
import {Link} from './Link';
import {tracker} from './analytics';
import {T} from './i18n';
import {localStorageBackedAtom, writeAtom} from './jotaiUtils';
import {applicationinfo} from './serverAPIState';
import {layout} from './stylexUtils';

const dismissedAlerts = localStorageBackedAtom<{[key: string]: boolean}>(
  'isl.dismissed-alerts',
  {},
);

const activeAlerts = atom<Array<Alert>>([]);

const ALERT_FETCH_INTERVAL_MS = 5 * 60 * 1000;

const alertsAlreadyLogged = new Set<string>();

serverAPI.onMessageOfType('fetchedActiveAlerts', event => {
  writeAtom(activeAlerts, event.alerts);
});
serverAPI.onSetup(() => {
  const fetchAlerts = () =>
    serverAPI.postMessage({
      type: 'fetchActiveAlerts',
    });
  const interval = setInterval(fetchAlerts, ALERT_FETCH_INTERVAL_MS);
  fetchAlerts();
  return () => clearInterval(interval);
});

export function TopLevelAlerts() {
  const [dismissed, setDismissed] = useAtom(dismissedAlerts);
  const alerts = useAtomValue(activeAlerts);
  const info = useAtomValue(applicationinfo);
  const version = info?.version;

  useEffect(() => {
    for (const {key} of alerts) {
      if (!alertsAlreadyLogged.has(key)) {
        tracker.track('AlertShown', {extras: {key}});
        alertsAlreadyLogged.add(key);
      }
    }
  }, [alerts]);

  return (
    <div>
      {alerts
        .filter(
          alert =>
            dismissed[alert.key] !== true &&
            (alert['isl-version-regex'] == null ||
              (version != null && new RegExp(alert['isl-version-regex']).test(version))),
        )
        .map((alert, i) => (
          <TopLevelAlert
            alert={alert}
            key={i}
            onDismiss={() => {
              setDismissed(old => ({...old, [alert.key]: true}));
              tracker.track('AlertDismissed', {extras: {key: alert.key}});
            }}
          />
        ))}
    </div>
  );
}

const styles = stylex.create({
  alertContainer: {
    margin: spacing.pad,
    position: 'relative',
  },
  alert: {
    fontSize: font.bigger,
    padding: spacing.pad,
    gap: spacing.half,
    alignItems: 'flex-start',
  },
  alertContent: {
    verticalAlign: 'center',
    gap: spacing.half,
    fontWeight: 'bold',
  },
  sev: {
    color: 'white',
    paddingInline: spacing.half,
    paddingBlock: spacing.quarter,
    borderRadius: radius.small,
    fontSize: font.small,
  },
  dismissX: {
    position: 'absolute',
    right: spacing.double,
    top: spacing.pad,
  },
  'SEV 0': {backgroundColor: colors.purple},
  'SEV 1': {backgroundColor: colors.red},
  'SEV 2': {backgroundColor: colors.orange},
  'SEV 3': {backgroundColor: colors.blue},
  'SEV 4': {backgroundColor: colors.grey},
  UBN: {backgroundColor: colors.purple},
});

function SevBadge({children, severity}: {children: ReactNode; severity: AlertSeverity}) {
  return <span {...stylex.props(styles.sev, styles[severity])}>{children}</span>;
}

function TopLevelAlert({alert, onDismiss}: {alert: Alert; onDismiss: () => unknown}) {
  const {title, description, url, severity} = alert;
  return (
    <div {...stylex.props(styles.alertContainer)}>
      <Banner kind={BannerKind.default} icon={<Icon icon="flame" size="M" color="red" />}>
        <div {...stylex.props(layout.flexCol, styles.alert)}>
          <div {...stylex.props(styles.dismissX)}>
            <Button onClick={onDismiss} data-testid="dismiss-alert">
              <Icon icon="x" />
            </Button>
          </div>
          <b>
            <T>Ongoing Issue</T>
          </b>
          <span {...stylex.props(layout.flexRow, styles.alertContent)}>
            <SevBadge severity={severity}>{severity}</SevBadge> <Link href={url}>{title}</Link>
            <Icon icon="link-external" />
          </span>
          <Subtle>{description}</Subtle>
        </div>
      </Banner>
    </div>
  );
}
