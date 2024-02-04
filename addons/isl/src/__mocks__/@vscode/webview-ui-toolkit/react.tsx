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
import type {FormEvent, JSXElementConstructor, MutableRefObject} from 'react';

import React, {useEffect, useRef, forwardRef} from 'react';

// vscode webview-ui-toolkit uses ES Modules, which doesn't play well with jest transpilation yet.
// We need to provide mock verison of these components for now

export const VSCodeLink = (p: React.PropsWithChildren<typeof VSCodeTagType>) => <a {...p} />;
export const VSCodeTag = (p: React.PropsWithChildren<typeof VSCodeTagType>) => <div {...p} />;
export const VSCodeBadge = (p: React.PropsWithChildren<typeof VSCodeBadgeType>) => <div {...p} />;
export const VSCodeButton = forwardRef(
  (
    p: React.PropsWithChildren<typeof VSCodeButtonType>,
    ref: React.LegacyRef<HTMLButtonElement>,
  ) => <button {...p} ref={ref} />,
);
export const VSCodeDivider = (p: React.PropsWithChildren<typeof VSCodeDividerType>) => (
  <div {...p} />
);

export const VSCodeCheckbox = (
  p: {onChange?: () => void; indeterminate?: boolean} & React.PropsWithChildren<
    typeof VSCodeCheckboxType
  >,
) => {
  const ref = useRef(null);
  const {onChange, children, indeterminate, ...rest} = p;
  useEffect(() => {
    if (indeterminate && ref.current) {
      (ref.current as HTMLInputElement).indeterminate = indeterminate;
    }
  }, [indeterminate, ref]);
  return (
    <div>
      <input ref={ref} type="checkbox" {...rest} onChange={onChange ?? (() => undefined)} />
      <label>{children}</label>
    </div>
  );
};
export const VSCodeTextField = forwardRef<HTMLInputElement>(
  (p: {children?: React.ReactNode; onChange?: () => void}, ref) => {
    const {children, onChange, ...rest} = p;
    return (
      <>
        {children && <label>{children}</label>}
        <input type="text" {...rest} ref={ref} onChange={onChange ?? (() => undefined)} />
      </>
    );
  },
);

export const VSCodeTextArea = forwardRef<HTMLDivElement>(
  (
    p: {
      children?: React.ReactNode;
      onChange?: () => void;
      className?: string;
      'data-testid'?: string;
    },
    ref,
  ) => {
    const {children, className, ['data-testid']: dataid, ...innerProps} = p;
    const outerProps = {className, ['data-testid']: dataid};

    const backupOuterRef = useRef(null);
    const outerRef = (ref ?? backupOuterRef) as MutableRefObject<HTMLDivElement>;
    const innerRef = useRef(null);

    // The actual VSCodeTextArea is a shadow element.
    // jest and react testing library don't handle those well,
    // so we mimic it with a div and a nested textarea.
    // we need to take care to be able to reference the inner textarea part via `.control`,
    // or else our testing code wouldn't match production.
    useEffect(() => {
      if (outerRef?.current && innerRef.current) {
        (outerRef.current as HTMLDivElement & {control: HTMLElement}).control = innerRef.current;
      }
    }, [outerRef, innerRef]);
    return (
      <div ref={outerRef} {...outerProps}>
        <label>{children}</label>
        <textarea
          // eslint-disable-next-line @typescript-eslint/ban-ts-comment
          // @ts-ignore - part is not usually allowed
          part="control"
          ref={innerRef}
          {...innerProps}
          onChange={innerProps.onChange ?? (() => undefined)}></textarea>
      </div>
    );
  },
);
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
