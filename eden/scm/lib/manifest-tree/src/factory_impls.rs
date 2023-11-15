/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use storemodel::Bytes;
use storemodel::SerializationFormat;
use storemodel::StaticSerializedTreeParseFunc;
use storemodel::TreeEntry;

use crate::store::Entry;

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
