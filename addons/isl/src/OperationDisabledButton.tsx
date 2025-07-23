/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PrimitiveAtom} from 'jotai';
import type {ComponentProps} from 'react';
import type {Operation} from './operations/Operation';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {atom, useAtom} from 'jotai';
import {isPromise} from 'shared/utils';
import {atomFamilyWeak} from './jotaiUtils';
import {useRunOperation} from './operationsState';
import {useMostRecentPendingOperation} from './previews';

/**
 * Wrapper around VSCodeButton intended for buttons which runOperations.
 * It remembers what Operation it spawns, and leaves the button disabled
 * if that operation is the most recent pending operation (queued or running).
 * If any further operations have been queued, then button will be re-enabled
 * (to allow queueing it again which may be valid).
 *
 * Note: do not use "useRunOperation" directly in the "runOperation", instead return the operation instance.
 *
 * runOperation may also return an Array of operations, if it queues multiple.
 * If the pending operation is ANY of those operations, the button will be disabled.
 *
 * Provide a `contextKey` to describe what this button is doing, to correlate with its resulting operation.
 * Generally this is just the name of the operation, but for operations that e.g. depend on a commit,
 * it may also include the commit hash so not every instance of this button is disabled.
 */
export function OperationDisabledButton({
  contextKey,
  runOperation,
  disabled,
  children,
  icon,
  ...rest
}: {
  contextKey: string;
  runOperation: () =>
    | Operation
    | Array<Operation>
    | undefined
    | Promise<Operation | Array<Operation> | undefined>;
  children?: React.ReactNode;
  disabled?: boolean;
  icon?: React.ReactNode;
  className?: string;
} & (Omit<ComponentProps<typeof Button>, 'icon' | 'primary'> & {kind?: string})) {
  const actuallyRunOperation = useRunOperation();
  const pendingOperation = useMostRecentPendingOperation();
  const [triggeredOperationId, setTriggeredOperationId] = useAtom(
    operationButtonDisableState(contextKey),
  );
  const isRunningThisOperation =
    pendingOperation != null && triggeredOperationId?.includes(pendingOperation.id);

  return (
    <Button
      {...rest}
      disabled={isRunningThisOperation || disabled}
      onClick={async () => {
        const opOrOpsResult = runOperation();
        let opOrOps;
        if (isPromise(opOrOpsResult)) {
          opOrOps = await opOrOpsResult;
        } else {
          opOrOps = opOrOpsResult;
        }
        if (opOrOps == null) {
          return;
        }
        const ops = Array.isArray(opOrOps) ? opOrOps : [opOrOps];
        for (const op of ops) {
          actuallyRunOperation(op);
        }
        setTriggeredOperationId(ops.map(op => op.id));
      }}>
      {isRunningThisOperation ? <Icon icon="loading" slot="start" /> : (icon ?? null)}
      {children}
    </Button>
  );
}

const operationButtonDisableState = atomFamilyWeak<
  string,
  PrimitiveAtom<Array<string> | undefined>
>((_param: string | undefined) => atom<Array<string> | undefined>(undefined));
