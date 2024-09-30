/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ClientToServerMessage, ServerToClientMessage} from '../types';

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Row} from 'isl-components/Flex';
import {ThemedComponentsRoot} from 'isl-components/ThemedComponentsRoot';
import vscodeApi from 'isl/src/vscodeSingleton';
import React, {useEffect} from 'react';
import ReactDOM from 'react-dom/client';

import 'isl-components/theme/themeDarkVariables.css';
import 'isl-components/theme/themeLightVariables.css';

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
  return (
    <React.StrictMode>
      <ThemedComponentsRoot theme={'light'}>
        <Row xstyle={style.alignTop}>
          <Counter />
          <DangerousHTML html={window.islCommentHtml} />
        </Row>
      </ThemedComponentsRoot>
    </React.StrictMode>
  );
}

function DangerousHTML({html}: {html: string}) {
  return <span dangerouslySetInnerHTML={{__html: html}} />;
}

function Counter() {
  const [count, setCount] = React.useState(2);
  useEffect(() => {
    window.addEventListener('message', event => {
      const message = event.data as ServerToClientMessage;
      switch (message.type) {
        case 'gotSquared':
          setCount(message.result);
          break;
      }
    });
  }, []);

  return (
    <>
      Count: {count}
      <Button
        xstyle={style.smallBtn}
        icon
        onClick={() => {
          vscodeApi?.postMessage({type: 'squareIt', value: count} as ClientToServerMessage);
        }}>
        Square It
      </Button>
    </>
  );
}
