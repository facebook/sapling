/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import DrawDagInput from "@site/src/components/DrawDagInput";
import React from 'react';
import RenderDag from "@site/src/components/RenderDag";

export default function DrawDagExample({initValue, showParents = false}) {
  const [state, setState] = React.useState(() => { return {
    parents: new Map(),
    dag: null,
    subset: null,
  }});

  // Limit nodes to show if there are too many.
  function getSubset({input, parents, dag, bindings}) {
    const limit = 10;
    let subset = null;
    const orderOverride = input.match(/# order: (.*)/);
    if (orderOverride) {
      const names = orderOverride[1].split(' ');
      subset = new bindings.JsSet(names);
    } else if (parents.size > limit) {
      const allNames = [...parents.keys()];
      const keyNames = allNames.filter((k) => input.indexOf(k) >= 0);
      subset = dag.sort(new bindings.JsSet(keyNames));
      while (subset.count() < limit) {
        let lastCount = subset.count();
        // Include "adjacent" nodes.
        subset = subset.union(dag.parents(subset)).union(dag.children(dag.heads(subset)));
        if (lastCount === subset.count()) {
          break;
        }
      }
    }
    return subset;
  }

  function getDrawExtra({dag}) {
    if (!showParents || !dag) {
      return null;
    }
    return function drawParentLabels({circles, r, updateViewbox, xyt}) {
      const labels = [];
      for (const [name, {cx, cy}] of circles) {
        const parents = dag.parentNames(name);
        if (parents.length > 1 || parents.some((p) => (circles.get(p) ?? {}).cy !== cy)) {
          const prefix = parents.length > 1 ? 'parents' : 'parent';
          const text = `${prefix}: ${parents.join(', ')}`;
          const x = cx;
          const y = cy + r + 2;
          labels.push(<text x={x} y={y} textAnchor="middle" alignmentBaseline="hanging" fontSize="0.7em" key={name}>{text}</text>);
          updateViewbox(x - 50, y + 10);
          updateViewbox(x + 50, y + 10);
        }
      }
      return <g fill="var(--ifm-color-content)">{labels}</g>;
    }
  }

  function onDagParentsChange({input, parents, bindings}) {
    setState((prevState) => {
      let {dag, subset} = prevState;
      const newDag = new bindings.JsDag();
      try {
        newDag.addHeads(parents, []);
        dag = newDag;
        subset = getSubset({input, dag, parents, bindings});
      } catch (ex) {
        console.error(ex);
      }
      return {...prevState, dag, parents, subset};
    });
  }

  const containerStyle = {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
  };

  const renderDagStyle = {
    padding: 'var(--ifm-alert-padding-vertical) var(--ifm-alert-padding-horizontal)',
  };

  const columnWidth = showParents ? 22 : 14;

  return <div className="drawdag row" style={containerStyle}>
    <div className="col col--6">
      <DrawDagInput initValue={initValue} onDagParentsChange={onDagParentsChange} />
    </div>
    <div className="col col--6" style={renderDagStyle}>
      <RenderDag
        dag={state.dag}
        subset={state.subset}
        drawExtra={getDrawExtra(state)}
        columnWidth={columnWidth}
      />
    </div>
  </div>;
};
