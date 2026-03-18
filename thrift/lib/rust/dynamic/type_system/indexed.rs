/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use crate::TypeSystem;
use crate::error::InvalidTypeError;
use crate::nodes::EnumNode;
use crate::nodes::OpaqueAliasNode;
use crate::nodes::StructNode;
use crate::nodes::UnionNode;
use crate::type_ref::DefinitionRef;
use crate::type_ref::TypeRef;

/// Internal storage for definition nodes.
#[allow(dead_code)] // Constructed by the builder in the next commit.
pub(crate) enum DefinitionNode {
    Struct(Arc<StructNode>),
    Union(Arc<UnionNode>),
    Enum(Arc<EnumNode>),
    OpaqueAlias(Arc<OpaqueAliasNode>),
}

impl std::fmt::Debug for DefinitionNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Struct(node) => f.debug_tuple("Struct").field(&node.uri).finish(),
            Self::Union(node) => f.debug_tuple("Union").field(&node.uri).finish(),
            Self::Enum(node) => f.debug_tuple("Enum").field(&node.uri).finish(),
            Self::OpaqueAlias(node) => f.debug_tuple("OpaqueAlias").field(&node.uri).finish(),
        }
    }
}

impl DefinitionNode {
    pub(crate) fn to_definition_ref(&self) -> DefinitionRef {
        match self {
            Self::Struct(n) => DefinitionRef::Struct(Arc::clone(n)),
            Self::Union(n) => DefinitionRef::Union(Arc::clone(n)),
            Self::Enum(n) => DefinitionRef::Enum(Arc::clone(n)),
            Self::OpaqueAlias(n) => DefinitionRef::OpaqueAlias(Arc::clone(n)),
        }
    }

    pub(crate) fn to_type_ref(&self) -> TypeRef {
        match self {
            Self::Struct(n) => TypeRef::Struct(Arc::clone(n)),
            Self::Union(n) => TypeRef::Union(Arc::clone(n)),
            Self::Enum(n) => TypeRef::Enum(Arc::clone(n)),
            Self::OpaqueAlias(n) => TypeRef::OpaqueAlias(Arc::clone(n)),
        }
    }
}

/// A [`TypeSystem`] implementation backed by in-memory hash-map indexes.
#[derive(Debug)]
pub struct IndexedTypeSystem {
    pub(crate) definitions: HashMap<String, DefinitionNode>,
}

impl IndexedTypeSystem {
    #[allow(dead_code)] // Called by the builder in the next commit.
    pub(crate) fn new(definitions: HashMap<String, DefinitionNode>) -> Self {
        Self { definitions }
    }
}

impl TypeSystem for IndexedTypeSystem {
    fn get(&self, uri: &str) -> Option<DefinitionRef> {
        self.definitions.get(uri).map(|n| n.to_definition_ref())
    }

    fn known_uris(&self) -> HashSet<&str> {
        self.definitions.keys().map(|s| s.as_str()).collect()
    }

    fn user_defined(&self, uri: &str) -> Result<TypeRef, InvalidTypeError> {
        self.definitions
            .get(uri)
            .map(|n| n.to_type_ref())
            .ok_or_else(|| InvalidTypeError::UnknownUri(uri.to_string()))
    }
}
