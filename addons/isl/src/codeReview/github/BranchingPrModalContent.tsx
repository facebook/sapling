/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from '../../operations/Operation';
import type {CommitInfo} from '../../types';
import type {GithubUICodeReviewProvider} from './github';

import {Avatar} from '../../Avatar';
import {Commit} from '../../Commit';
import {Column, FlexSpacer, Row} from '../../ComponentUtils';
import {T} from '../../i18n';
import {CommitPreview} from '../../previews';
import * as stylex from '@stylexjs/stylex';
import {Badge} from 'isl-components/Badge';
import {Button} from 'isl-components/Button';
import {Checkbox} from 'isl-components/Checkbox';
import {Dropdown} from 'isl-components/Dropdown';
import {useState} from 'react';

const styles = stylex.create({
  sectionTitle: {
    fontWeight: 'bold',
  },
});

function getPushChoices(provider: GithubUICodeReviewProvider) {
  const system = provider.system;
  return [
    `${system.hostname === 'github.com' ? '' : system.hostname + '/'}${system.owner}/${
      system.repo
    }`,
  ];
}

export default function BranchingPrModalContent({
  stack,
  provider,
  returnResultAndDismiss,
}: {
  stack: Array<CommitInfo>;
  provider: GithubUICodeReviewProvider;
  returnResultAndDismiss: (result: Array<Operation> | undefined) => unknown;
}) {
  const [createPr, setCreatePr] = useState(false);

  const pushChoices = getPushChoices(provider);
  const [pushChoice, setPushChoice] = useState(pushChoices[0]);

  return (
    <Column alignStart style={{height: '100%'}}>
      <div>
        <Row {...stylex.props(styles.sectionTitle)}>
          <span>
            <T>Commits</T>
          </span>
          <Badge>{stack.length}</Badge>
        </Row>
        <Column alignStart style={{padding: 'var(--pad)'}}>
          {stack.map((commit, i) => (
            <Row key={i}>
              <Avatar username={commit.author} />
              <Commit
                commit={commit}
                previewType={CommitPreview.NON_ACTIONABLE_COMMIT}
                hasChildren={false}
              />
            </Row>
          ))}
        </Column>
      </div>
      <Row>
        <span>
          <T>Push to</T>
        </span>
        <Dropdown
          options={pushChoices}
          value={pushChoice}
          onChange={e => setPushChoice(e.currentTarget.value)}
        />
      </Row>
      <Row>
        <Checkbox checked={createPr} onChange={setCreatePr} disabled /* not implemented yet */>
          <T>Create a Pull Request</T>
        </Checkbox>
      </Row>

      <FlexSpacer />
      <Row style={{width: '100%'}}>
        <FlexSpacer />
        <Button onClick={() => returnResultAndDismiss(undefined)}>
          <T>Cancel</T>
        </Button>
        <Button primary>{createPr ? <T>Push & Create PR</T> : <T>Push</T>}</Button>
      </Row>
    </Column>
  );
}
