/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import React from 'react';
import Editor from 'react-simple-code-editor';
import equal from 'deep-equal';
import importBindings from "@site/src/utils/importBindings";

export default function DrawDagInput({initValue, style, padding = 10, onDagParentsChange}) {
  const [state, setState] = React.useState(() => { return {
    input: (initValue ?? '').replace(/^\n+|\s+$/g, ''),
    parents: new Map(),
    comments: '',
    bindings: null,
    dropped: false,
  }});

  React.useEffect(() => {
    importBindings().then((bindings) => {
      if (!state.bindings && !state.dropped) {
        setState((state) => { return {...state, bindings}; });
      }
    }).catch(console.error);
    return function cleanup() {
      state.dropped = true;
    }
  }, []);

  // Trigger onDagParentsChange.
  // useEffect avoids https://reactjs.org/link/setstate-in-render
  React.useEffect(() => {
    // This function should not cause re-render.
    // state is modified directly without setState.
    const {bindings, input, dropped, comments} = state;
    if (!bindings || dropped) {
      return;
    }
    if (onDagParentsChange) {
      try {
        const inputWithoutComments = input.replace(/#.*$/mg, '');
        const newComments = input.replace(/.*#$/mg, '');
        const newParents = bindings.drawdag(inputWithoutComments);
        if (!equal(state.parents, newParents) || comments !== newComments) {
          onDagParentsChange({parents: newParents, bindings, input});
          state.parents = newParents;
          state.comments = newComments;
        }
      } catch (ex) {
        console.error(ex);
      }
    }
    return function cleanup() {
      state.dropped = true;
    }
  }, [state.bindings, state.input]);

  function onValueChange(input) {
    setState((prevState) => {
      if (prevState.input === input && prevState.parents) {
        // Avoids re-render.
        return prevState;
      }
      return {...prevState, input};
    });
  }

  const mergedStyle = {
    background: 'var(--ifm-color-emphasis-100)',
    borderRadius: 'var(--ifm-global-radius)',
    fontFamily: 'var(--ifm-font-family-monospace)',
    lineHeight: 1,
    ...style
  };

  return <Editor
    value={state.input}
    highlight={(c) => c}
    padding={padding}
    style={mergedStyle}
    onValueChange={onValueChange}
  />
}
