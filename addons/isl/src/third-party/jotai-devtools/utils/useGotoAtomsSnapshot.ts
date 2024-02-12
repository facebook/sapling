import { useCallback } from 'react';
import { useStore } from 'jotai/react';
import type { AtomsSnapshot, Options } from '../types';

export function useGotoAtomsSnapshot(options?: Options) {
  const store = useStore(options);
  return useCallback(
    (snapshot: AtomsSnapshot) => {
      if (store.dev_restore_atoms) {
        store.dev_restore_atoms(snapshot.values);
      }
    },
    [store],
  );
}
