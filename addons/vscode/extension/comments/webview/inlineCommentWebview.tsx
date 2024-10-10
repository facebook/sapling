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
import React, {useEffect, useRef, useState} from 'react';
import ReactDOM from 'react-dom/client';
import * as Comparison from 'shared/Comparison';

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
    initialEditorLineHeight: number;
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

  const minHeightInLines = 5;
  const initialEditorLineHeight = window.initialEditorLineHeight ?? 8;

  const ref = useObserveElementHeight<HTMLDivElement>(initialEditorLineHeight, minHeightInLines);

  return (
    <React.StrictMode>
      <div
        ref={ref}
        style={{
          display: 'flex',
          flexDirection: 'column',
          width: '1000px',
          minHeight: initialEditorLineHeight * minHeightInLines,
          paddingTop: '3px',
          paddingBottom: '6px',
          boxSizing: 'border-box',
        }}>
        <ThemedComponentsRoot theme={'dark'}>
          <Row xstyle={style.alignTop}>
            <SetHeightComponent />
            {path && codeSuggestion && (
              <InlineCommentComparisonView
                path={path}
                suggestion={codeSuggestion}
                ctx={{
                  collapsed: false,
                  id: {
                    comparison: {type: Comparison.ComparisonType.HeadChanges},
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
      </div>
    </React.StrictMode>
  );
}

const SetHeightComponent = () => {
  const [height, setHeight] = useState(100);
  const [inputHeight, setInputHeight] = useState('');

  const handleHeightChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setInputHeight(event.target.value);
  };

  const submitHeight = () => {
    const newHeight = parseInt(inputHeight, 10);
    if (!isNaN(newHeight)) {
      setHeight(newHeight);
    }
  };

  return (
    <div style={{display: 'flex', flexDirection: 'column', alignItems: 'center', height}}>
      <input
        type="number"
        value={inputHeight}
        onChange={handleHeightChange}
        style={{margin: '10px'}}
      />
      <button onClick={submitHeight} style={{margin: '10px'}}>
        Set Height
      </button>
    </div>
  );
};

/**
 * Automatically resize the chat to fit its content.
 * @returns A ref to attach to the root element to measure.
 */
export const useObserveElementHeight = <T extends HTMLElement>(
  lineHeight: number,
  minLines: number,
) => {
  const [height, setHeight] = useState(0);
  const ref = useRef<T>(null);

  useEffect(() => {
    const currentElement = ref.current;
    const observer = new ResizeObserver(entries => {
      setHeight(entries[0].contentRect.height);
    });
    if (currentElement) {
      observer.observe(currentElement);
    }
    return () => {
      // TODO: QUESTION!! I just got back using React so not an expert.
      // I got warning saying ref.current might change when I clean up in the return statement.
      // And told me to refernece it into a variable. Will this cause any memory leaks if the
      // ref did indeed change? The unobserve will still clean up the ref right?
      if (currentElement) {
        observer.unobserve(currentElement);
      }
    };
  }, []);

  useEffect(() => {
    const heightInPx = Math.ceil(height ?? 0);
    const lines = Math.max(minLines, heightInPx / lineHeight);
    const linesRounded = Math.round(lines);
    vscodeApi?.postMessage({type: 'setInsetHeight', height: linesRounded});
  });

  return ref;
};
