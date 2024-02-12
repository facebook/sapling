import { useEffect, useRef } from 'react';
import { AnyAtom, AnyAtomValue, AtomsSnapshot } from '../types';
import {
  Connection,
  createReduxConnection,
} from './redux-extension/createReduxConnection';
import { getReduxExtension } from './redux-extension/getReduxExtension';
import { SnapshotOptions, useAtomsSnapshot } from './useAtomsSnapshot';
import { useGotoAtomsSnapshot } from './useGotoAtomsSnapshot';

const atomToPrintable = (atom: AnyAtom) =>
  atom.debugLabel ? `${atom}:${atom.debugLabel}` : `${atom}`;

const getDevtoolsState = (atomsSnapshot: AtomsSnapshot) => {
  const values: Record<string, AnyAtomValue> = {};
  atomsSnapshot.values.forEach((v, atom) => {
    values[atomToPrintable(atom)] = v;
  });
  const dependents: Record<string, string[]> = {};
  atomsSnapshot.dependents.forEach((d, atom) => {
    dependents[atomToPrintable(atom)] = Array.from(d).map(atomToPrintable);
  });
  return {
    values,
    dependents,
  };
};

type DevtoolsOptions = SnapshotOptions & {
  enabled?: boolean;
};

export function useAtomsDevtools(
  name: string,
  options?: DevtoolsOptions,
): void {
  const { enabled } = options || {};

  const extension = getReduxExtension(enabled);

  // This an exception, we don't usually use utils in themselves!
  const atomsSnapshot = useAtomsSnapshot(options);
  const goToSnapshot = useGotoAtomsSnapshot(options);

  const isTimeTraveling = useRef(false);
  const isRecording = useRef(true);
  const devtools = useRef<Connection>();

  const snapshots = useRef<AtomsSnapshot[]>([]);

  useEffect(() => {
    if (!extension) {
      return;
    }
    const getSnapshotAt = (index = snapshots.current.length - 1) => {
      // index 0 is @@INIT, so we need to return the next action (0)
      const snapshot = snapshots.current[index >= 0 ? index : 0];
      if (!snapshot) {
        throw new Error('snapshot index out of bounds');
      }
      return snapshot;
    };

    devtools.current = createReduxConnection(extension, name);

    const devtoolsUnsubscribe = devtools.current?.subscribe((message) => {
      switch (message.type) {
        case 'DISPATCH':
          switch (message.payload?.type) {
            case 'RESET':
              // TODO
              break;

            case 'COMMIT':
              devtools.current?.init(getDevtoolsState(getSnapshotAt()));
              snapshots.current = [];
              break;

            case 'JUMP_TO_ACTION':
            case 'JUMP_TO_STATE':
              isTimeTraveling.current = true;
              goToSnapshot(getSnapshotAt(message.payload.actionId - 1));
              break;

            case 'PAUSE_RECORDING':
              isRecording.current = !isRecording.current;
              break;
          }
      }
    });

    return () => {
      extension?.disconnect?.();
      devtoolsUnsubscribe?.();
    };
  }, [extension, goToSnapshot, name]);

  useEffect(() => {
    if (!devtools.current) {
      return;
    }
    if (devtools.current.shouldInit) {
      devtools.current.init(undefined);
      devtools.current.shouldInit = false;
      return;
    }
    if (isTimeTraveling.current) {
      isTimeTraveling.current = false;
    } else if (isRecording.current) {
      snapshots.current.push(atomsSnapshot);
      devtools.current.send(
        {
          type: `${snapshots.current.length}`,
          updatedAt: new Date().toLocaleString(),
        } as any,
        getDevtoolsState(atomsSnapshot),
      );
    }
  }, [atomsSnapshot]);
}
