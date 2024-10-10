/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import 'isl/src/ComparisonView/SplitDiffView/SplitDiffHunk.css';

import type {ServerToClientMessage} from '../types';
import type {DiffComment} from 'isl/src/types';

import InlineCommentComparisonView from './InlineCommentComparisonView';
import * as stylex from '@stylexjs/stylex';
import {Row} from 'isl-components/Flex';
import {ThemedComponentsRoot} from 'isl-components/ThemedComponentsRoot';
import vscodeApi from 'isl/src/vscodeSingleton';
import React, {useEffect, useState} from 'react';
import ReactDOM from 'react-dom/client';
import {ComparisonType} from 'shared/Comparison';

import 'isl-components/theme/themeDarkVariables.css';
import 'isl-components/theme/themeLightVariables.css';
import 'isl-components/theme/themeDark.css';
import 'isl-components/theme/themeLight.css';

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(<App />);

const style = stylex.create({
  smallBtn: {
    padding: '0 5px',
  },
  alignTop: {alignItems: 'flex-start'},
});

declare global {
  interface Window {
    islCommentHtml: string;
  }
}

function App() {
  const [comment, setComment] = useState<{diffComment: DiffComment; hash: string}>();

  useEffect(() => {
    window.addEventListener('message', event => {
      const message = event.data as ServerToClientMessage;
      switch (message.type) {
        case 'fetchedDiffComment':
          setComment({
            hash: message.hash,
            diffComment: message.comment,
          });
          break;
      }
    });
  }, []);

  useEffect(() => {
    vscodeApi?.postMessage({type: 'fetchDiffComment'});
  }, []);

  const diffComment = comment?.diffComment;
  const path = diffComment?.filename ?? '';
  const codeSuggestion = diffComment?.suggestedChange ?? null;

  return (
    <React.StrictMode>
      <ThemedComponentsRoot theme={'dark'}>
        <Row xstyle={style.alignTop}>
          {path && codeSuggestion && (
            <InlineCommentComparisonView
              path={path}
              suggestion={codeSuggestion}
              ctx={{
                collapsed: false,
                id: {
                  comparison: {type: ComparisonType.HeadChanges},
                  path,
                },
                setCollapsed: () => null,
                supportsExpandingContext: false,
                display: 'unified',
              }}
            />
          )}
        </Row>
      </ThemedComponentsRoot>
    </React.StrictMode>
  );
}
