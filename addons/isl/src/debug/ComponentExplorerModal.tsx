/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StyleXVar} from '@stylexjs/stylex/lib/StyleXTypes';
import type {ReactNode} from 'react';

import {Banner, BannerKind} from '../Banner';
import {ErrorNotice} from '../ErrorNotice';
import {Link} from '../Link';
import {Tooltip} from '../Tooltip';
import {VSCodeCheckbox} from '../VSCodeCheckbox';
import {T} from '../i18n';
import {layout} from '../stylexUtils';
import {colors, font, radius, spacing} from '../tokens.stylex';
import * as stylex from '@stylexjs/stylex';
import {
  VSCodeBadge,
  VSCodeButton,
  VSCodeDivider,
  VSCodeDropdown,
  VSCodeOption,
  VSCodeRadio,
  VSCodeTag,
  VSCodeTextArea,
  VSCodeTextField,
} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';

const basicBgs = ['bg', 'subtleHoverDarken', 'hoverDarken'] as const;
const pureColors = ['red', 'yellow', 'orange', 'green', 'blue', 'purple', 'grey'] as const;
const scmColors = ['modifiedFg', 'addedFg', 'removedFg', 'missingFg'] as const;
const signalColors = ['signalGoodBg', 'signalMediumBg', 'signalBadBg'] as const;
const paddings = ['none', 'quarter', 'half', 'pad', 'double', 'xlarge'] as const;
const fontSizes = ['smaller', 'small', 'normal', 'big', 'bigger'] as const;

export default function ComponentExplorer(_: {dismiss: (_: unknown) => unknown}) {
  return (
    <div {...stylex.props(styles.container)}>
      <h2>
        <T>Component Explorer</T>
      </h2>
      <div {...stylex.props(styles.container, layout.flexCol, layout.fullWidth)}>
        <GroupName>Colors</GroupName>
        <Row>
          {basicBgs.map(name => (
            <Badge fg={colors.fg} bg={colors[name]} key={name}>
              {name}
            </Badge>
          ))}
        </Row>
        <Row>
          {scmColors.map(name => (
            <Badge fg={colors[name]} bg={colors.bg} key={name}>
              <Icon icon="diff-modified" />
              {name}
            </Badge>
          ))}
        </Row>
        <Row>
          {pureColors.map(name => (
            <Badge fg={colors[name]} bg={colors.bg} key={name}>
              {name}
            </Badge>
          ))}
        </Row>
        <Row>
          {pureColors.map(name => (
            <Badge fg={colors.fg} bg={colors[name]} key={name}>
              {name}
            </Badge>
          ))}
        </Row>
        <Row>
          <Badge fg={colors.errorFg} bg={colors.errorBg}>
            Error
          </Badge>
          {signalColors.map(name => (
            <Badge fg={colors.signalFg} bg={colors[name]} key={name}>
              {name}
            </Badge>
          ))}
        </Row>
        <GroupName>Components</GroupName>
        <Row>
          <VSCodeButton>Primary</VSCodeButton>
          <VSCodeButton appearance="secondary">Secondary</VSCodeButton>
          <VSCodeButton appearance="icon">Icon</VSCodeButton>
          <VSCodeButton appearance="icon">
            <Icon icon="rocket" slot="start" /> Icon
          </VSCodeButton>
          <VSCodeButton appearance="icon">
            <Icon icon="rocket" />
          </VSCodeButton>
          <VSCodeDropdown>
            <VSCodeOption>Dropdown</VSCodeOption>
            <VSCodeOption>Option</VSCodeOption>
          </VSCodeDropdown>
        </Row>
        <Row>
          <VSCodeCheckbox>Checkbox</VSCodeCheckbox>
          <VSCodeCheckbox checked>Checked</VSCodeCheckbox>
          <VSCodeCheckbox disabled>Disabled</VSCodeCheckbox>
          <VSCodeRadio>Radio</VSCodeRadio>
          <VSCodeRadio checked>Selected</VSCodeRadio>
          <VSCodeRadio disabled>Disabled</VSCodeRadio>
        </Row>
        <Row>
          <VSCodeBadge>Badge</VSCodeBadge>
          <VSCodeBadge>0</VSCodeBadge>
          <VSCodeTag>Tag</VSCodeTag>
          <VSCodeTag>0</VSCodeTag>
          <Link href={'#'}>Link</Link>
          <Icon icon="loading" />
          Loading
        </Row>
        <VSCodeDivider />
        <Row>
          <VSCodeTextArea placeholder="placeholder">Text area</VSCodeTextArea>
          <VSCodeTextField placeholder="placeholder">Text Field</VSCodeTextField>
          <Tooltip trigger="manual" shouldShow={true} title="Tooltip" placement="bottom">
            Thing
          </Tooltip>
        </Row>

        <Row>
          <Banner>Banner</Banner>
          <Banner kind={BannerKind.warning}>Warning Banner</Banner>
          <Banner kind={BannerKind.error}>Error Banner</Banner>
          <Banner icon={<Icon icon="info" />}>Icon Banner</Banner>
        </Row>
        <Row>
          <ErrorNotice
            title="Error Notice"
            description="description"
            details="details / stack trace"
          />
        </Row>
        <GroupName>Spacing</GroupName>
        <Row>
          {paddings.map(size => (
            <Badge style={styles.padding(size)} key={size}>
              {size}
            </Badge>
          ))}
        </Row>
        <Row>
          <div {...stylex.props(layout.flexCol)} style={{alignItems: 'flex-start'}}>
            {paddings.map(size => (
              <div {...stylex.props(layout.flexRow)} style={{gap: spacing[size]}}>
                <Badge>A</Badge>
                <Badge>B</Badge>
                <Badge>{size}</Badge>
              </div>
            ))}
          </div>
        </Row>
        <GroupName>Font</GroupName>
        <Row>
          {fontSizes.map(size => (
            <Badge style={styles.font(size)} bg={colors.hoverDarken} key={size}>
              {size}
            </Badge>
          ))}
        </Row>
      </div>
    </div>
  );
}

const styles = stylex.create({
  container: {
    padding: spacing.pad,
    overflow: 'auto',
  },
  badge: (fg, bg) => ({
    backgroundColor: bg,
    color: fg,
    fontFamily: 'monospace',
    paddingBlock: spacing.half,
    paddingInline: spacing.pad,
    borderRadius: radius.round,
  }),
  groupName: {
    fontSize: font.bigger,
    width: '100%',
    paddingTop: spacing.double,
    fontWeight: 'bold',
  },
  padding: (pad: (typeof paddings)[number]) => ({
    padding: spacing[pad],
  }),
  font: (size: (typeof fontSizes)[number]) => ({
    fontSize: font[size],
  }),
});

function Badge({
  children,
  bg,
  fg,
  style,
}: {
  children: ReactNode;
  bg?: StyleXVar<string>;
  fg?: StyleXVar<string>;
  style?: stylex.StyleXStyles;
}) {
  return (
    <div {...stylex.props(layout.flexRow, styles.badge(fg, bg ?? colors.hoverDarken), style)}>
      {children}
    </div>
  );
}

function Row({children, style}: {children: ReactNode; style?: stylex.StyleXStyles}) {
  return <div {...stylex.props(layout.flexRow, layout.fullWidth, style)}>{children}</div>;
}

function GroupName({children}: {children: ReactNode}) {
  return <div {...stylex.props(styles.groupName)}>{children}</div>;
}
