import { useEffect, useRef } from 'react';
import { useAtom } from 'jotai/react';
import type { Atom, WritableAtom } from 'jotai/vanilla';
import {
  Connection,
  createReduxConnection,
} from './redux-extension/createReduxConnection';
import { getReduxExtension } from './redux-extension/getReduxExtension';

type DevtoolOptions = Parameters<typeof useAtom>[1] & {
  name?: string;
  enabled?: boolean;
};

export function useAtomDevtools<Value, Result>(
  anAtom: WritableAtom<Value, [Value], Result> | Atom<Value>,
  options?: DevtoolOptions,
): void {
  const { enabled, name } = options || {};

  const extension = getReduxExtension(enabled);

  const [value, setValue] = useAtom(anAtom, options);

  const lastValue = useRef(value);
  const isTimeTraveling = useRef(false);
  const devtools = useRef<Connection>();

  const atomName = name || anAtom.debugLabel || anAtom.toString();

  useEffect(() => {
    if (!extension) {
      return;
    }
    const setValueIfWritable = (value: Value) => {
      if (typeof setValue === 'function') {
        (setValue as (value: Value) => void)(value);
        return;
      }
      console.warn(
        '[Warn] you cannot do write operations (Time-travelling, etc) in read-only atoms\n',
        anAtom,
      );
    };

    devtools.current = createReduxConnection(extension, atomName);

    const unsubscribe = devtools.current?.subscribe((message) => {
      if (message.type === 'ACTION' && message.payload) {
        try {
          setValueIfWritable(JSON.parse(message.payload));
        } catch (e) {
          console.error(
            'please dispatch a serializable value that JSON.parse() support\n',
            e,
          );
        }
      } else if (message.type === 'DISPATCH' && message.state) {
        if (
          message.payload?.type === 'JUMP_TO_ACTION' ||
          message.payload?.type === 'JUMP_TO_STATE'
        ) {
          isTimeTraveling.current = true;

          setValueIfWritable(JSON.parse(message.state));
        }
      } else if (
        message.type === 'DISPATCH' &&
        message.payload?.type === 'COMMIT'
      ) {
        devtools.current?.init(lastValue.current);
      } else if (
        message.type === 'DISPATCH' &&
        message.payload?.type === 'IMPORT_STATE'
      ) {
        const computedStates =
          message.payload.nextLiftedState?.computedStates || [];

        computedStates.forEach(({ state }: { state: Value }, index: number) => {
          if (index === 0) {
            devtools.current?.init(state);
          } else {
            setValueIfWritable(state);
          }
        });
      }
    });

    return unsubscribe;
  }, [anAtom, extension, atomName, setValue]);

  useEffect(() => {
    if (!devtools.current) {
      return;
    }
    lastValue.current = value;
    if (devtools.current.shouldInit) {
      devtools.current.init(value);
      devtools.current.shouldInit = false;
    } else if (isTimeTraveling.current) {
      isTimeTraveling.current = false;
    } else {
      devtools.current.send(
        `${atomName} - ${new Date().toLocaleString()}` as any,
        value,
      );
    }
  }, [anAtom, extension, atomName, value]);
}
