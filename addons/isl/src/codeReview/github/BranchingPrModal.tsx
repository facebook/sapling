/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from '../../operations/Operation';
import type {CommitInfo} from '../../types';
import type {GithubUICodeReviewProvider} from './github';

import {T} from '../../i18n';
import {readAtom} from '../../jotaiUtils';
import {dagWithPreviews} from '../../previews';
import {showModal} from '../../useModal';
import {codeReviewProvider} from '../CodeReviewInfo';
import {ErrorNotice} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {useAtomValue} from 'jotai';
import {lazy, Suspense} from 'react';

const BranchingPrModalContent = lazy(() => import('./BranchingPrModalContent'));

export function showBranchingPrModal(
  topOfStackToPush: CommitInfo,
): Promise<Array<Operation> | undefined> {
  const dag = readAtom(dagWithPreviews);
  const stack = dag.getBatch(dag.ancestors(topOfStackToPush.hash, {within: dag.draft()}).toArray());
  return showModal<Array<Operation> | undefined>({
    maxWidth: 'calc(min(90vw, 800px)',
    maxHeight: 'calc(min(90vw, 600px)',
    width: 'inherit',
    height: 'inherit',
    type: 'custom',
    dataTestId: 'create-pr-modal',
    component: ({returnResultAndDismiss}) => (
      <CreatePrModal stack={stack} returnResultAndDismiss={returnResultAndDismiss} />
    ),
  });
}

export function CreatePrModal({
  stack,
  returnResultAndDismiss,
}: {
  stack: Array<CommitInfo>;
  returnResultAndDismiss: (operations: Array<Operation> | undefined) => unknown;
}) {
  const provider = useAtomValue(codeReviewProvider);
  if (provider == null || provider.name !== 'github') {
    return (
      <ErrorNotice
        title="Unsupported Code Review Provider"
        description={`Found provider: ${provider?.name}`}
      />
    );
  }
  return (
    <Suspense fallback={<Icon icon="loading" size="M" />}>
      <div id="use-modal-title">
        <Icon icon={'repo-push'} size="M" />
        <T>Push & Create Pull Request</T>
      </div>
      <BranchingPrModalContent
        provider={provider as GithubUICodeReviewProvider}
        stack={stack}
        returnResultAndDismiss={returnResultAndDismiss}
      />
    </Suspense>
  );
}
