/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StyleXVar} from '@stylexjs/stylex/lib/StyleXTypes';

import * as stylex from '@stylexjs/stylex';
import {useState, type ReactNode} from 'react';
import {Badge} from '../Badge';
import {Banner, BannerKind} from '../Banner';
import {Button} from '../Button';
import {ButtonDropdown} from '../ButtonDropdown';
import {ButtonGroup} from '../ButtonGroup';
import {Checkbox} from '../Checkbox';
import {Divider} from '../Divider';
import {Dropdown} from '../Dropdown';
import {ErrorNotice} from '../ErrorNotice';
import {HorizontallyGrowingTextField} from '../HorizontallyGrowingTextField';
import {Icon} from '../Icon';
import {Kbd} from '../Kbd';
import {KeyCode, Modifier} from '../KeyboardShortcuts';
import {Panels} from '../Panels';
import {RadioGroup} from '../Radio';
import {Subtle} from '../Subtle';
import {Tag} from '../Tag';
import {TextArea} from '../TextArea';
import {TextField} from '../TextField';
import {Tooltip} from '../Tooltip';
import {Typeahead} from '../Typeahead';
import {layout} from '../theme/layout';
import {colors, font, radius, spacing} from '../theme/tokens.stylex';

/* eslint-disable no-console */

const basicBgs = ['bg', 'subtleHoverDarken', 'hoverDarken'] as const;
const pureColors = ['red', 'yellow', 'orange', 'green', 'blue', 'purple', 'grey'] as const;
const scmColors = ['modifiedFg', 'addedFg', 'removedFg', 'missingFg'] as const;
const signalColors = ['signalGoodBg', 'signalMediumBg', 'signalBadBg'] as const;
const paddings = ['none', 'quarter', 'half', 'pad', 'double', 'xlarge'] as const;
const fontSizes = ['smaller', 'small', 'normal', 'big', 'bigger'] as const;

export default function ComponentExplorer() {
  const [radioChoice, setRadioChoice] = useState('radio');
  const [checkbox1, setCheckbox1] = useState(false);
  const [checkbox2, setCheckbox2] = useState(true);
  const [dropdownChoice, setDropdownChoice] = useState('B');
  const buttonDropdownOptions = [
    {
      id: 'action 1',
      label: 'Action 1',
    },
    {
      id: 'action 2',
      label: 'Action 2',
    },
  ];
  const [activePanel, setActivePanel] = useState<'fruit' | 'vegetables'>('fruit');
  const [buttonDropdownChoice, setButtonDropdownChoice] = useState(buttonDropdownOptions[0]);
  return (
    <div {...stylex.props(styles.container)}>
      <h2>Component Explorer</h2>
      <div {...stylex.props(styles.container, layout.flexCol, layout.fullWidth)}>
        <GroupName>Colors</GroupName>
        <Row>
          Normal
          <Subtle>Subtle</Subtle>
        </Row>
        <Row>
          {basicBgs.map(name => (
            <ColorBadge fg={colors.fg} bg={colors[name]} key={name}>
              {name}
            </ColorBadge>
          ))}
        </Row>
        <Row>
          {scmColors.map(name => (
            <ColorBadge fg={colors[name]} bg={colors.bg} key={name}>
              <Icon icon="diff-modified" />
              {name}
            </ColorBadge>
          ))}
        </Row>
        <Row>
          {pureColors.map(name => (
            <ColorBadge fg={colors[name]} bg={colors.bg} key={name}>
              {name}
            </ColorBadge>
          ))}
        </Row>
        <Row>
          {pureColors.map(name => (
            <ColorBadge fg={colors.fg} bg={colors[name]} key={name}>
              {name}
            </ColorBadge>
          ))}
        </Row>
        <Row>
          <ColorBadge fg={colors.errorFg} bg={colors.errorBg}>
            Error
          </ColorBadge>
          {signalColors.map(name => (
            <ColorBadge fg={colors.signalFg} bg={colors[name]} key={name}>
              {name}
            </ColorBadge>
          ))}
        </Row>
        <Row>
          <span style={{border: '1px solid var(--focus-border)'}}>Focus border</span>
          <span style={{border: '1px solid var(--contrast-border)'}}>Contrast Border</span>
          <span style={{border: '1px solid var(--contrast-active-border)'}}>
            Contrast Active Border
          </span>
        </Row>
        <Row>
          <Icon icon="info" />
          <Icon icon="pass" color="green" />
          <Icon icon="warning" color="yellow" />
          <Icon icon="error" color="red" />
          <Icon icon="lightbulb" color="blue" />
        </Row>
        <Row>
          XS:
          <Icon icon="rocket" size="XS" />
          <span> </span>
          S: (default)
          <Icon icon="rocket" size="S" />
          <span> </span>
          M:
          <Icon icon="rocket" size="M" />
          <span> </span>
          L:
          <Icon icon="rocket" size="L" />
        </Row>
        <GroupName>Components</GroupName>
        <Row>
          <Button primary>Primary</Button>
          <Button disabled primary>
            Primary
          </Button>
          <Button>Secondary</Button>
          <Button disabled>Secondary</Button>
          <Button icon>Icon</Button>
          <Button icon>
            <Icon icon="rocket" />
            Icon
          </Button>
          <Button icon>
            <Icon icon="rocket" />
          </Button>
          <Button icon disabled>
            <Icon icon="rocket" /> Icon
          </Button>
          <Button>
            <Icon icon="rocket" /> Secondary With Icon
          </Button>
        </Row>
        <Row>
          <Dropdown
            options={['Dropdown', 'Option']}
            onChange={e => console.log(e.currentTarget.value)}
          />
          <Dropdown
            disabled
            options={[
              {value: 'none', name: 'Disabled Option', disabled: true},
              {value: 'drop', name: 'Dropdown'},
              {value: 'opt', name: 'Option'},
            ]}
            onChange={e => console.log(e.currentTarget.value)}
          />
          <Dropdown
            value={dropdownChoice}
            onChange={e => setDropdownChoice(e.currentTarget.value)}
            options={['A', 'B', 'C']}
          />
        </Row>
        <Row>
          <ButtonDropdown
            icon={<Icon icon="rocket" />}
            options={buttonDropdownOptions}
            selected={buttonDropdownChoice}
            onClick={selected => console.log('click!', selected)}
            onChangeSelected={setButtonDropdownChoice}
          />
          <ButtonDropdown
            options={buttonDropdownOptions}
            buttonDisabled
            selected={buttonDropdownChoice}
            onClick={selected => console.log('click!', selected)}
            onChangeSelected={setButtonDropdownChoice}
          />
          <ButtonDropdown
            options={buttonDropdownOptions}
            pickerDisabled
            selected={buttonDropdownChoice}
            onClick={selected => console.log('click!', selected)}
            onChangeSelected={setButtonDropdownChoice}
          />
          <ButtonDropdown
            icon={<Icon icon="rocket" />}
            kind="icon"
            options={buttonDropdownOptions}
            selected={buttonDropdownChoice}
            onClick={selected => console.log('click!', selected)}
            onChangeSelected={setButtonDropdownChoice}
          />
          <ButtonDropdown
            kind="icon"
            options={buttonDropdownOptions}
            buttonDisabled
            selected={buttonDropdownChoice}
            onClick={selected => console.log('click!', selected)}
            onChangeSelected={setButtonDropdownChoice}
          />
          <ButtonDropdown
            kind="icon"
            options={buttonDropdownOptions}
            pickerDisabled
            selected={buttonDropdownChoice}
            onClick={selected => console.log('click!', selected)}
            onChangeSelected={setButtonDropdownChoice}
          />
        </Row>
        <Row>
          <ButtonGroup>
            <Button>A</Button>
            <Tooltip title="Wrapping in a tooltip doesn't affect the group styling">
              <Button>B</Button>
            </Tooltip>
            <Button>
              <Icon icon="close" />
            </Button>
          </ButtonGroup>
          <ButtonGroup
            icon /* Be sure to set icon=True on the group if the buttons are icon=True */
          >
            <Button icon style={{paddingInline: '5px'}}>
              Action A
            </Button>
            <Button icon style={{paddingInline: '5px'}}>
              Action B
            </Button>
            <Button icon>
              <Icon icon="close" />
            </Button>
          </ButtonGroup>
          <ButtonGroup>
            <Button>A</Button>
            <Button disabled>B</Button>
            <Button>C</Button>
            <Button primary>D</Button>
            <Button>
              <Icon icon="close" />
            </Button>
          </ButtonGroup>
        </Row>
        <Row>
          <Checkbox checked={checkbox1} onChange={setCheckbox1}>
            Checkbox
          </Checkbox>
          <Checkbox checked={checkbox2} onChange={setCheckbox2}>
            Checked
          </Checkbox>
          <Checkbox checked={false} indeterminate onChange={console.log}>
            Indeterminate
          </Checkbox>
          <Checkbox checked disabled onChange={setCheckbox1}>
            Disabled
          </Checkbox>
          <RadioGroup
            choices={[
              {title: 'Radio', value: 'radio'},
              {title: 'Another', value: 'another'},
            ]}
            current={radioChoice}
            onChange={setRadioChoice}
          />
        </Row>
        <Row>
          Kbd:
          <Kbd keycode={KeyCode.A} modifiers={[Modifier.CMD]} />
        </Row>
        <Row>
          <Badge>Badge</Badge>
          <Badge>0</Badge>
          <Tag>Tag</Tag>
          <Tag>0</Tag>
          {/* <Link href={'#'}>Link</Link> */}
          <Icon icon="loading" />
          Loading
        </Row>
        <Divider />
        <Row>
          <TextArea placeholder="placeholder" onChange={e => console.log(e.currentTarget.value)}>
            Text area
          </TextArea>
          <TextField placeholder="placeholder" onChange={e => console.log(e.currentTarget.value)}>
            Text Field
          </TextField>
          <Tooltip trigger="manual" shouldShow={true} title="Tooltip" placement="bottom">
            Thing
          </Tooltip>
        </Row>
        <Row>
          <HorizontallyGrowingTextField
            placeholder="grows as you type"
            onInput={e => console.log(e.currentTarget.value)}
          />
        </Row>
        <Row>
          <span>Typeahead:</span>
          <ExampleTypeahead />
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
        <Row>
          <Panels
            active={activePanel}
            panels={{
              fruit: {label: 'Fruit', render: () => <div>Apple</div>},
              vegetables: {label: 'Vegetables', render: () => <div>Broccoli</div>},
            }}
            onSelect={setActivePanel}
          />
        </Row>
        <GroupName>Spacing</GroupName>
        <Row>
          {paddings.map(size => (
            <ColorBadge style={styles.padding(size)} key={size}>
              {size}
            </ColorBadge>
          ))}
        </Row>
        <Row>
          <div {...stylex.props(layout.flexCol)} style={{alignItems: 'flex-start'}}>
            {paddings.map(size => (
              <div {...stylex.props(layout.flexRow)} style={{gap: spacing[size]}} key={size}>
                <ColorBadge>A</ColorBadge>
                <ColorBadge>B</ColorBadge>
                <ColorBadge>{size}</ColorBadge>
              </div>
            ))}
          </div>
        </Row>
        <GroupName>Font</GroupName>
        <Row>
          {fontSizes.map(size => (
            <ColorBadge style={styles.font(size)} bg={colors.hoverDarken} key={size}>
              {size}
            </ColorBadge>
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

function ColorBadge({
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

function ExampleTypeahead() {
  const [value, setValue] = useState('');

  const possibleValues = [
    'apple',
    'banana',
    'cherry',
    'date',
    'elderberry',
    'fig',
    'grape',
    'honeydew',
    'jackfruit',
    'kiwi',
  ];
  const fetchTokens = async (searchTerm: string) => {
    await new Promise(resolve => setTimeout(resolve, 500));
    return {
      values: possibleValues
        .filter(v => v.includes(searchTerm))
        .map(value => ({
          label: value,
          value,
        })),
      fetchStartTimestamp: Date.now(),
    };
  };
  return (
    <Typeahead
      tokenString={value}
      setTokenString={setValue}
      fetchTokens={fetchTokens}
      autoFocus={false}
      maxTokens={3}
    />
  );
}
