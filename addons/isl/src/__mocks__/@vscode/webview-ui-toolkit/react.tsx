/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  VSCodeButton as VSCodeButtonType,
  VSCodeCheckbox as VSCodeCheckboxType,
  VSCodeTag as VSCodeTagType,
  VSCodeDivider as VSCodeDividerType,
  VSCodeBadge as VSCodeBadgeType,
  VSCodeRadio as VSCodeRadioType,
  VSCodeRadioGroup as VSCodeRadioGroupType,
} from '@vscode/webview-ui-toolkit/react';
import type {FormEvent, JSXElementConstructor} from 'react';

import React, {forwardRef} from 'react';

// vscode webview-ui-toolkit uses ES Modules, which doesn't play well with jest transpilation yet.
// We need to provide mock verison of these components for now

export const VSCodeTag = (p: React.PropsWithChildren<typeof VSCodeTagType>) => <div {...p} />;
export const VSCodeBadge = (p: React.PropsWithChildren<typeof VSCodeBadgeType>) => <div {...p} />;
export const VSCodeButton = (p: React.PropsWithChildren<typeof VSCodeButtonType>) => (
  <button {...p} />
);
export const VSCodeDivider = (p: React.PropsWithChildren<typeof VSCodeDividerType>) => (
  <div {...p} />
);

export const VSCodeCheckbox = (p: React.PropsWithChildren<typeof VSCodeCheckboxType>) => (
  <input type="checkbox" {...p} onChange={() => undefined} />
);
export const VSCodeTextField = forwardRef<HTMLInputElement>((p, ref) => (
  <input type="text" {...p} ref={ref} />
));
export const VSCodeTextArea = forwardRef<HTMLTextAreaElement>((p, ref) => (
  <textarea {...p} ref={ref} />
));
export const VSCodeDropdown = forwardRef<HTMLSelectElement>((p, ref) => (
  <select {...p} ref={ref} />
));
export const VSCodeOption = forwardRef<HTMLOptionElement>((p, ref) => <option {...p} ref={ref} />);

export const VSCodeRadio = ({
  children,
  onClick,
  ...p
}: React.PropsWithChildren<typeof VSCodeRadioType> & {onClick: (e: FormEvent) => unknown}) => (
  <div onClick={onClick}>
    <input
      type="radio"
      {...p}
      // React complains at runtime if we provide `checked` to an <input> without `onChange`
      onChange={() => undefined}
    />
    {children}
  </div>
);
// We need to emulate VSCodeRadioGroup's onChange in order to test it correctly.
// Just trigger the RadioGroup's `onChange` in each Radio's `onClick`.
export const VSCodeRadioGroup = ({
  children,
  onChange,
  ...p
}: React.PropsWithChildren<typeof VSCodeRadioGroupType> & {
  onChange: (e: FormEvent) => unknown;
}) => {
  // assume the children of a RadioGroup are Radios...
  const radios = (children as Array<JSXElementConstructor<typeof VSCodeRadioType>>).map(
    (child, i) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const props = (child as any).props;
      return (
        <VSCodeRadio
          {...props}
          onClick={(_: FormEvent) => onChange({target: props} as FormEvent)}
          key={i}
        />
      );
    },
  );
  return <fieldset {...p}>{radios}</fieldset>;
};
