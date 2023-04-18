/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';
import type {CommitMessageFields, FieldConfig, FieldsBeingEdited} from './types';
import type {ReactNode} from 'react';

import {YouAreHere} from '../Commit';
import {Subtle} from '../Subtle';
import {Tooltip} from '../Tooltip';
import {T} from '../i18n';
import {RelativeDate} from '../relativeDate';

export function CommitTitleByline({commit}: {commit: CommitInfo}) {
  const createdByInfo = (
    // TODO: determine if you're the author to say "you"
    <T replace={{$author: commit.author}}>Created by $author</T>
  );
  return (
    <Subtle className="commit-info-title-byline">
      {commit.isHead ? <YouAreHere hideSpinner /> : null}
      <OverflowEllipsis shrink>
        <Tooltip trigger="hover" component={() => createdByInfo}>
          {createdByInfo}
        </Tooltip>
      </OverflowEllipsis>
      <OverflowEllipsis>
        <Tooltip trigger="hover" title={commit.date.toLocaleString()}>
          <RelativeDate date={commit.date} />
        </Tooltip>
      </OverflowEllipsis>
    </Subtle>
  );
}

export function OverflowEllipsis({children, shrink}: {children: ReactNode; shrink?: boolean}) {
  return <div className={`overflow-ellipsis${shrink ? ' overflow-shrink' : ''}`}>{children}</div>;
}

export function SmallCapsTitle({children}: {children: ReactNode}) {
  return <div className="commit-info-small-title">{children}</div>;
}

export function Section({
  children,
  className,
  ...rest
}: React.DetailedHTMLProps<React.HTMLAttributes<HTMLElement>, HTMLElement>) {
  return (
    <section {...rest} className={'commit-info-section' + (className ? ' ' + className : '')}>
      {children}
    </section>
  );
}

export function getTopmostEditedField(
  fields: Array<FieldConfig>,
  fieldsBeingEdited: FieldsBeingEdited,
): keyof CommitMessageFields | undefined {
  for (const field of fields) {
    if (fieldsBeingEdited[field.key]) {
      return field.key;
    }
  }
  return undefined;
}

/**
 * VSCodeTextArea elements use custom components, which renders in a shadow DOM.
 * Most often, we want to access the inner <textarea>, which acts like a normal textarea.
 */
export function getInnerTextareaForVSCodeTextArea(
  outer: HTMLElement | null,
): HTMLTextAreaElement | null {
  return outer == null ? null : (outer as unknown as {control: HTMLTextAreaElement}).control;
}
