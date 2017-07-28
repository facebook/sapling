// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::ops::Deref;

use juniper::Value;

use mercurial_types::NodeHash;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct GQLNodeId(NodeHash);

impl From<NodeHash> for GQLNodeId {
    fn from(nodehash: NodeHash) -> Self {
        GQLNodeId(nodehash)
    }
}

impl<'a> From<&'a NodeHash> for GQLNodeId {
    fn from(nodehash: &'a NodeHash) -> Self {
        GQLNodeId(*nodehash)
    }
}

impl Deref for GQLNodeId {
    type Target = NodeHash;

    fn deref(&self) -> &NodeHash {
        &self.0
    }
}

graphql_scalar!(GQLNodeId as "NodeId" {
    description: "Unique ID of a particular version of a file."

    resolve(&self) -> Value {
        Value::string(format!("{}", &self.0))
    }

    from_input_value(v: &InputValue) -> Option<GQLNodeId> {
        v.as_string_value().and_then(|s| s.parse().ok()).map(GQLNodeId)
    }
});
