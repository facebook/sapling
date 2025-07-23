/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from '../../operations/Operation';
import type {CommitInfo} from '../../types';
import type {GithubUICodeReviewProvider} from './github';

import * as stylex from '@stylexjs/stylex';
import {Badge} from 'isl-components/Badge';
import {Button} from 'isl-components/Button';
import {Checkbox} from 'isl-components/Checkbox';
import {Dropdown} from 'isl-components/Dropdown';
import {HorizontallyGrowingTextField} from 'isl-components/HorizontallyGrowingTextField';
import {useAtomValue} from 'jotai';
import {useState} from 'react';
import {Avatar} from '../../Avatar';
import {Commit} from '../../Commit';
import {Column, FlexSpacer, Row} from '../../ComponentUtils';
import {T} from '../../i18n';
import {PushOperation} from '../../operations/PushOperation';
import {CommitPreview, dagWithPreviews} from '../../previews';
import {latestSuccessorUnlessExplicitlyObsolete} from '../../successionUtils';

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

function recommendNewBranchName(stack: Array<CommitInfo>) {
  // TODO: this format should be configurable
  return `feature/${stack[0].title
    .trim()
    .replace(/\s/g, '-')
    .replace(/[^a-zA-Z0-9\-_.]/g, '.')
    .replace(/^\.+/, '') // no leading dots
    .replace(/\/$/, '') // no trailing slashes
    .toLowerCase()}`;
}

export default function BranchingPrModalContent({
  topOfStack,
  provider,
  returnResultAndDismiss,
}: {
  topOfStack: CommitInfo;
  provider: GithubUICodeReviewProvider;
  returnResultAndDismiss: (result: Array<Operation> | undefined) => unknown;
}) {
  const [createPr, setCreatePr] = useState(false);

  const dag = useAtomValue(dagWithPreviews);
  // If passed the optimistic isDot commit, we may need to resolve it to a real commit
  // once the optimistic commit is no longer in the dag.
  const top =
    topOfStack.isDot && topOfStack.optimisticRevset != null
      ? (dag.resolve('.') ?? topOfStack)
      : topOfStack;
  const stack = dag.getBatch(dag.ancestors(top.hash, {within: dag.draft()}).toArray());

  const pushChoices = getPushChoices(provider);
  const [pushChoice, setPushChoice] = useState(pushChoices[0]);

  const [branchName, setBranchName] = useState(recommendNewBranchName(stack));

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
          <T>Push to repo</T>
        </span>
        <Dropdown
          options={pushChoices}
          value={pushChoice}
          onChange={e => setPushChoice(e.currentTarget.value)}
        />
      </Row>
      <Row>
        <span>
          <T>to branch named</T>
        </span>
        <HorizontallyGrowingTextField
          value={branchName}
          onChange={e => setBranchName(e.currentTarget.value)}
        />
        {/* TODO: validate the branch name */}
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
        <Button
          primary
          onClick={() => {
            if (createPr) {
              throw new Error('not implemented');
            }

            returnResultAndDismiss([
              new PushOperation(latestSuccessorUnlessExplicitlyObsolete(stack[0]), branchName),
            ]);
          }}>
          {createPr ? <T>Push & Create PR</T> : <T>Push</T>}
        </Button>
      </Row>
    </Column>
  );
}
