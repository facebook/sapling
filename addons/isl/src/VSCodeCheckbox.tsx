/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Constructable} from '@microsoft/fast-element';

import * as OriginalToolkit from '@vscode/webview-ui-toolkit/react';
import {useRef} from 'react';

/**
 * A patched version of VSCodeCheckbox. Do not trigger `onChange` if `checked`
 * is managed and changed by other places.
 *
 * See https://github.com/microsoft/vscode-webview-ui-toolkit/issues/408.
 */
export function VSCodeCheckbox(props: VSCodeCheckboxProps) {
  // props.checked from the last render.
  const ignoreChecked = useRef<boolean | null>(null);
  if (ignoreChecked.current !== props.checked && props.checked != null) {
    ignoreChecked.current = props.checked;
  }

  // Replace onChange handler to ignore managed 'checked' changed by re-render.
  const onChange: VSCodeCheckboxProps['onChange'] =
    props.onChange == null
      ? undefined
      : (...args) => {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          const checked = (args[0].target as any).checked;
          if (ignoreChecked.current == checked) {
            return;
          }
          // This seems obviously correct yet tsc cannot type check it.
          // eslint-disable-next-line @typescript-eslint/ban-ts-comment
          // @ts-ignore
          return props.onChange(...args);
        };

  return <OriginalToolkit.VSCodeCheckbox {...props} onChange={onChange} />;
}

type ExtractProps<T> = T extends Constructable<React.Component<infer P, unknown, unknown>>
  ? P
  : never;
type VSCodeCheckboxProps = ExtractProps<typeof OriginalToolkit.VSCodeCheckbox>;
