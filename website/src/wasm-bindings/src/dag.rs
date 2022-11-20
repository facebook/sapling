/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::Arc;

use dag::nameset::SyncNameSetQuery;
use dag::ops::DagAddHeads;
use dag::ops::DagAlgorithm;
use dag::ops::IdConvert;
use dag::ops::IdDagAlgorithm;
use dag::Group;
use dag::Id;
use dag::IdSet;
use dag::MemDag;
use dag::Set;
use dag::Vertex;
use dag::VertexListWithOptions;
use nonblocking::non_blocking_result as r;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::Convert;
use crate::JsResult;

#[wasm_bindgen]
pub struct JsDag(MemDag);

#[wasm_bindgen]
pub struct JsIdDag(Arc<dyn IdDagAlgorithm + Send + Sync>);

#[wasm_bindgen]
pub struct JsSet(Set);

#[wasm_bindgen]
pub struct JsIdSet(IdSet);

// Like dag::IdSegment but use i32 instead of Id/u64.
// Also see Convert between Id and i32 below.
//
// Id converts to u64 natively. But i64/u64 can cause WASM errors like:
//
//   ERROR in ./src/wasm-bindings/pkg/wasm_bindings_bg.wasm
//   Import "__wbindgen_bigint_from_u64" from "./wasm_bindings_bg.js" with Non-JS-compatible Func
//   Signature (i64 as parameter) can only be used for direct wasm to wasm dependencies
//
// i32 is used instead of u64 to make WASM happy.
#[derive(Serialize)]
struct IdSegment {
    low: i32,
    high: i32,
    parents: Vec<i32>,
    hasRoot: bool,
    level: u8,
}

#[wasm_bindgen]
impl JsDag {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(MemDag::new())
    }

    /// addHeads(parents: {name: [name]}, main_heads: [name])
    #[wasm_bindgen]
    pub fn addHeads(&mut self, parents: JsValue, main_heads: JsValue) -> JsResult<()> {
        let parents: HashMap<String, Vec<String>> = serde_wasm_bindgen::from_value(parents)?;
        let parents: HashMap<Vertex, Vec<Vertex>> = parents.convert();
        let heads: VertexListWithOptions = {
            let main_heads: Vec<String> = serde_wasm_bindgen::from_value(main_heads)?;
            let main_heads: Vec<Vertex> = main_heads.convert();
            let all_parents: BTreeSet<Vertex> = parents.values().flatten().cloned().collect();
            let all_children: BTreeSet<Vertex> =
                parents.keys().cloned().collect::<BTreeSet<Vertex>>();
            let all_heads: Vec<Vertex> = all_children.difference(&all_parents).cloned().collect();
            VertexListWithOptions::from(main_heads)
                .with_highest_group(Group::MASTER)
                .chain(all_heads)
        };
        r(self.0.add_heads(&parents, &heads))?;
        Ok(())
    }

    #[wasm_bindgen]
    pub fn render(&self) -> JsResult<JsValue> {
        let result = dag::render::render_namedag_structured(&self.0, None).unwrap();
        Ok(serde_wasm_bindgen::to_value(&result)?)
    }

    #[wasm_bindgen]
    pub fn renderSubset(&self, subset: &JsSet) -> JsResult<JsValue> {
        let result =
            dag::render::render_namedag_structured(&self.0, Some(subset.0.clone())).unwrap();
        Ok(serde_wasm_bindgen::to_value(&result)?)
    }

    // trait DagAlgorithm {{

    #[wasm_bindgen]
    pub fn parentNames(&self, name: String) -> JsResult<JsValue> {
        let name = name.convert();
        let names = r(self.0.parent_names(name))?;
        let names: Vec<String> = names.convert();
        Ok(serde_wasm_bindgen::to_value(&names)?)
    }

    #[wasm_bindgen]
    pub fn all(&self) -> JsResult<JsSet> {
        let set = r(self.0.all())?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn mainGroup(&self) -> JsResult<JsSet> {
        let set = r(self.0.master_group())?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn dirty(&self) -> JsResult<JsSet> {
        let set = r(self.0.dirty())?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn sort(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.sort(&set.0))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn ancestors(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.ancestors(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn parents(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.parents(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn firstAncestors(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.first_ancestors(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn heads(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.heads(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn children(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.children(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn roots(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.roots(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn merges(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.merges(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn gca(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.gca_all(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn commonAncestors(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.common_ancestors(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn headsAncestors(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.heads_ancestors(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn range(&self, roots: &JsSet, heads: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.range(roots.0.clone(), heads.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn only(&self, reachable: &JsSet, unreachable: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.only(reachable.0.clone(), unreachable.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn descendants(&self, set: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.descendants(set.0.clone()))?;
        Ok(JsSet(set))
    }

    #[wasm_bindgen]
    pub fn reachableRoots(&self, roots: &JsSet, heads: &JsSet) -> JsResult<JsSet> {
        let set = r(self.0.reachable_roots(roots.0.clone(), heads.0.clone()))?;
        Ok(JsSet(set))
    }

    // Gateway to low-level Ids.
    #[wasm_bindgen]
    pub fn idDag(&self) -> JsResult<JsIdDag> {
        let id_dag = self.0.id_dag_snapshot()?;
        Ok(JsIdDag(id_dag))
    }

    // }} // trait DagAlgorithm

    // trait IdConvert {{

    #[wasm_bindgen]
    pub fn vertexId(&self, name: String) -> JsResult<JsValue> {
        let id: Option<Id> = r(self.0.vertex_id_optional(&name.convert()))?;
        let id: Option<i32> = id.convert();
        Ok(serde_wasm_bindgen::to_value(&id)?)
    }

    #[wasm_bindgen]
    pub fn vertexName(&self, id: i32) -> JsResult<String> {
        let id: Id = id.convert();
        let name = r(self.0.vertex_name(id))?;
        let name: String = name.convert();
        Ok(name)
    }

    #[wasm_bindgen]
    pub fn convtainsVertexIdLocally(&self, ids: JsValue) -> JsResult<JsValue> {
        let ids: Vec<i32> = serde_wasm_bindgen::from_value(ids)?;
        let ids: Vec<Id> = ids.convert();
        let result: Vec<bool> = r(self.0.contains_vertex_id_locally(&ids))?;
        Ok(serde_wasm_bindgen::to_value(&result)?)
    }

    #[wasm_bindgen]
    pub fn convtainsVertexNameLocally(&self, names: JsValue) -> JsResult<JsValue> {
        let names: Vec<String> = serde_wasm_bindgen::from_value(names)?;
        let names: Vec<Vertex> = names.convert();
        let result: Vec<bool> = r(self.0.contains_vertex_name_locally(&names))?;
        Ok(serde_wasm_bindgen::to_value(&result)?)
    }

    // }} // trait IdConvert
}

#[wasm_bindgen]
impl JsIdDag {
    // trait IdDagAlgorithm {{

    #[wasm_bindgen]
    pub fn parentIds(&self, id: i32) -> JsResult<JsValue> {
        let id: Id = id.convert();
        let ids: Vec<Id> = self.0.parent_ids(id)?;
        let ids: Vec<i32> = ids.convert();
        Ok(serde_wasm_bindgen::to_value(&ids)?)
    }

    #[wasm_bindgen]
    pub fn all(&self) -> JsResult<JsIdSet> {
        let set = self.0.all()?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn mainGroup(&self) -> JsResult<JsIdSet> {
        let set = self.0.master_group()?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn ancestors(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.ancestors(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn parents(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.parents(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn firstAncestors(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.first_ancestors(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn heads(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.heads(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn children(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.children(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn roots(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.roots(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn merges(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.merges(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn gca(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.gca_all(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn commonAncestors(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.common_ancestors(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn headsAncestors(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.heads_ancestors(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn range(&self, roots: &JsIdSet, heads: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.range(roots.0.clone(), heads.0.clone())?;
        Ok(JsIdSet(set))
    }

    #[wasm_bindgen]
    pub fn descendants(&self, set: &JsIdSet) -> JsResult<JsIdSet> {
        let set = self.0.descendants(set.0.clone())?;
        Ok(JsIdSet(set))
    }

    // Gateway to get segments information.
    #[wasm_bindgen]
    pub fn segments(&self, set: &JsIdSet, max_level: u8) -> JsResult<JsValue> {
        let id_segments = self
            .0
            .id_set_to_id_segments_with_max_level(&set.0, max_level)?;
        let id_segments: Vec<IdSegment> = id_segments
            .into_iter()
            .map(|s| IdSegment {
                low: s.low.convert(),
                high: s.high.convert(),
                parents: s.parents.convert(),
                hasRoot: s.has_root,
                level: s.level,
            })
            .collect();
        Ok(serde_wasm_bindgen::to_value(&id_segments)?)
    }

    // }} // trait IdDagAlgorithm
}

#[wasm_bindgen]
impl JsSet {
    #[wasm_bindgen(constructor)]
    pub fn new(value: JsValue) -> JsResult<JsSet> {
        if value.is_null() || value.is_undefined() {
            Ok(Self(Set::empty()))
        } else {
            let names: Vec<String> = serde_wasm_bindgen::from_value(value)?;
            let names: Vec<Vertex> = names.convert();
            let set = Set::from_static_names(names);
            Ok(Self(set))
        }
    }

    #[wasm_bindgen]
    pub fn toString(&self) -> String {
        format!("{:?}", &self.0)
    }

    #[wasm_bindgen]
    pub fn toJSON(&self) -> JsResult<JsValue> {
        let names: Vec<Vertex> = SyncNameSetQuery::iter(&self.0)?.collect::<Result<Vec<_>, _>>()?;
        let names: Vec<String> = names.convert();
        Ok(serde_wasm_bindgen::to_value(&names)?)
    }

    #[wasm_bindgen]
    pub fn count(&self) -> JsResult<usize> {
        Ok(self.0.count()?)
    }

    #[wasm_bindgen]
    pub fn skip(&self, n: u32) -> JsResult<JsSet> {
        Ok(Self(self.0.skip(n as _)))
    }

    #[wasm_bindgen]
    pub fn take(&self, n: u32) -> JsResult<JsSet> {
        Ok(Self(self.0.take(n as _)))
    }

    #[wasm_bindgen]
    pub fn union(&self, rhs: &JsSet) -> JsResult<JsSet> {
        Ok(Self(self.0.union(&rhs.0)))
    }

    #[wasm_bindgen]
    pub fn intersection(&self, rhs: &JsSet) -> JsResult<JsSet> {
        Ok(Self(self.0.intersection(&rhs.0)))
    }

    #[wasm_bindgen]
    pub fn difference(&self, rhs: &JsSet) -> JsResult<JsSet> {
        Ok(Self(self.0.difference(&rhs.0)))
    }
}

#[wasm_bindgen]
impl JsIdSet {
    #[wasm_bindgen(constructor)]
    pub fn new(value: JsValue) -> JsResult<JsIdSet> {
        if value.is_null() || value.is_undefined() {
            Ok(Self(Default::default()))
        } else {
            let ids: Vec<i32> = serde_wasm_bindgen::from_value(value)?;
            let ids: Vec<Id> = ids.convert();
            let set = IdSet::from_spans(ids);
            Ok(Self(set))
        }
    }

    #[wasm_bindgen]
    pub fn toString(&self) -> String {
        format!("{:?}", &self.0)
    }

    #[wasm_bindgen]
    pub fn toJSON(&self) -> JsResult<JsValue> {
        let ids: Vec<Id> = self.0.iter_desc().collect();
        let ids: Vec<i32> = ids.convert();
        Ok(serde_wasm_bindgen::to_value(&ids)?)
    }

    #[wasm_bindgen]
    pub fn count(&self) -> JsResult<u32> {
        Ok(self.0.count() as _)
    }

    #[wasm_bindgen]
    pub fn skip(&self, n: u32) -> JsResult<JsIdSet> {
        Ok(Self(self.0.skip(n as _)))
    }

    #[wasm_bindgen]
    pub fn take(&self, n: u32) -> JsResult<JsIdSet> {
        Ok(Self(self.0.take(n as _)))
    }

    #[wasm_bindgen]
    pub fn union(&self, rhs: &JsIdSet) -> JsResult<JsIdSet> {
        Ok(Self(self.0.union(&rhs.0)))
    }

    #[wasm_bindgen]
    pub fn intersection(&self, rhs: &JsIdSet) -> JsResult<JsIdSet> {
        Ok(Self(self.0.intersection(&rhs.0)))
    }

    #[wasm_bindgen]
    pub fn difference(&self, rhs: &JsIdSet) -> JsResult<JsIdSet> {
        Ok(Self(self.0.difference(&rhs.0)))
    }
}
