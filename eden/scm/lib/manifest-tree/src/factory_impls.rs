/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use storemodel::Bytes;
use storemodel::SerializationFormat;
use storemodel::StaticSerializeTreeFunc;
use storemodel::StaticSerializedTreeParseFunc;
use storemodel::TreeEntry;
use storemodel::TreeItemFlag;
use types::Id20;
use types::PathComponentBuf;

use crate::store::Entry;
use crate::TreeElement;

pub(crate) fn setup_basic_tree_parser_constructor() {
    fn parse_tree(data: Bytes, format: SerializationFormat) -> anyhow::Result<Box<dyn TreeEntry>> {
        let entry = Entry(data, format);
        Ok(Box::new(entry))
    }
    fn construct_parse_tree(_: &()) -> anyhow::Result<Option<StaticSerializedTreeParseFunc>> {
        Ok(Some(parse_tree))
    }
    factory::register_constructor("99-basic", construct_parse_tree);
}

pub(crate) fn setup_basic_tree_serializer_constructor() {
    fn serialize_tree(
        items: Vec<(PathComponentBuf, Id20, TreeItemFlag)>,
        format: SerializationFormat,
    ) -> anyhow::Result<Bytes> {
        let elements: Vec<TreeElement> = items
            .into_iter()
            .map(|(component, hgid, flag)| TreeElement {
                component,
                hgid,
                flag,
            })
            .collect();
        Ok(Entry::from_elements(elements, format).to_bytes())
    }
    fn construct_serialize_tree(_: &()) -> anyhow::Result<Option<StaticSerializeTreeFunc>> {
        Ok(Some(serialize_tree))
    }
    factory::register_constructor("99-basic", construct_serialize_tree);
}
