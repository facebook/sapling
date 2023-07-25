/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {T} from '../i18n';

import './SuggestedReviewers.css';

export function SuggestedReviewers() {
  const suggested = ['muirdm', 'quark', 'person1', 'person2', 'person3'];
  const recent = ['#isl', 'quark', 'person1', 'person2', 'person3'];
  return (
    <div className="suggested-reviewers">
      <div>
        <div className="suggestion-header">
          <T>Suggested</T>
        </div>
        <div className="suggestions">
          {suggested.map(s => (
            <Suggestion key={s}>{s}</Suggestion>
          ))}
        </div>
      </div>
      {recent.length > 0 ? (
        <div>
          <div className="suggestion-header">
            <T>Recent</T>
          </div>
          <div className="suggestions">
            {recent.map(s => (
              <Suggestion key={s}>{s}</Suggestion>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function Suggestion({children}: {children: ReactNode}) {
  return <span className="suggestion token">{children}</span>;
}
