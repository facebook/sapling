import {
  useDebugValue,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { useStore } from 'jotai/react';
import type { Atom } from 'jotai/vanilla';

type Store = ReturnType<typeof useStore>;
type AtomState = NonNullable<
  ReturnType<NonNullable<Store['dev_get_atom_state']>>
>;

const atomToPrintable = (atom: Atom<unknown>) =>
  atom.debugLabel || atom.toString();

const stateToPrintable = ([store, atoms]: [Store, Atom<unknown>[]]) =>
  Object.fromEntries(
    atoms.flatMap((atom) => {
      const mounted = store.dev_get_mounted?.(atom);
      if (!mounted) {
        return [];
      }
      const dependents = mounted.t;
      const atomState = store.dev_get_atom_state?.(atom) || ({} as AtomState);
      return [
        [
          atomToPrintable(atom),
          {
            ...('e' in atomState && { error: atomState.e }),
            ...('v' in atomState && { value: atomState.v }),
            dependents: Array.from(dependents).map(atomToPrintable),
          },
        ],
      ];
    }),
  );

type Options = Parameters<typeof useStore>[0] & {
  enabled?: boolean;
};

// We keep a reference to the atoms,
// so atoms aren't garbage collected by the WeakMap of mounted atoms
export const useAtomsDebugValue = (options?: Options) => {
  const enabled = options?.enabled ?? __DEV__;
  const store = useStore(options);
  const [atoms, setAtoms] = useState<Atom<unknown>[]>([]);
  const duringReactRenderPhase = useRef(true);
  duringReactRenderPhase.current = true;
  useLayoutEffect(() => {
    duringReactRenderPhase.current = false;
  });
  useEffect(() => {
    const devSubscribeStore: Store['dev_subscribe_store'] =
      // @ts-expect-error dev_subscribe_state is deprecated in <= 2.0.3
      store?.dev_subscribe_store || store?.dev_subscribe_state;

    if (!enabled || !devSubscribeStore) {
      return;
    }
    const callback = () => {
      const deferrableAtomSetAction = () =>
        setAtoms(Array.from(store.dev_get_mounted_atoms?.() || []));
      if (duringReactRenderPhase.current) {
        // avoid set action when react is rendering components
        Promise.resolve().then(deferrableAtomSetAction);
      } else {
        deferrableAtomSetAction();
      }
    };
    // FIXME replace this with `store.dev_subscribe_store` check after next minor Jotai 2.1.0?
    if (!('dev_subscribe_store' in store)) {
      console.warn(
        "[DEPRECATION-WARNING] Jotai version you're using contains deprecated dev-only properties that will be removed soon. Please update to the latest version of Jotai.",
      );
    }

    const unsubscribe = devSubscribeStore?.(callback, 2);
    callback();
    return unsubscribe;
  }, [enabled, store]);
  useDebugValue([store, atoms], stateToPrintable);
};
