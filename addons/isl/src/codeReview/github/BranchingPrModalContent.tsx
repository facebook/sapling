/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from '../../operations/Operation';
import type {CommitInfo} from '../../types';

import {Avatar} from '../../Avatar';
import {Commit} from '../../Commit';
import {Column, Row} from '../../ComponentUtils';
import {T} from '../../i18n';
import {CommitPreview} from '../../previews';
import * as stylex from '@stylexjs/stylex';
import {Badge} from 'isl-components/Badge';

const styles = stylex.create({
  sectionTitle: {
    fontWeight: 'bold',
  },
});

export default function BranchingPrModalContent({
  stack,
}: {
  stack: Array<CommitInfo>;
  returnResultAndDismiss: (result: Array<Operation> | undefined) => unknown;
}) {
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
    </Column>
  );
}
