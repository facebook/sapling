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

//! In-memory representation of a Thrift type system.
//!
//! Models all Thrift type definitions — primitives, containers, and
//! user-defined types — as a shared, cheaply-cloneable type graph.
//! Designed for construction from (and round-tripping to)
//! [`SerializableTypeSystem`].

use std::collections::HashSet;
use std::sync::Arc;

pub mod containers;
pub mod error;
pub mod indexed;
pub mod nodes;
pub mod type_ref;

pub use containers::ListType;
pub use containers::MapType;
pub use containers::SetType;
pub use error::InvalidTypeError;
pub use indexed::IndexedTypeSystem;
pub use nodes::AnnotationsMap;
pub use nodes::EnumNode;
pub use nodes::EnumValue;
pub use nodes::FieldDefinition;
pub use nodes::FieldIdentity;
pub use nodes::OpaqueAliasNode;
pub use nodes::PresenceQualifier;
pub use nodes::StructNode;
pub use nodes::StructuredNode;
pub use nodes::UnionNode;
pub use type_ref::DefinitionRef;
pub use type_ref::Kind;
pub use type_ref::TypeRef;

/// `TypeSystem` provides a graph of runtime representations for
/// thrift types.
///
/// It supports:
/// - URI-based lookup of user-defined types, e.g. structs, unions, ...
/// - [`TypeId`](type_id::TypeId) resolution, mapping from a description of a type to its properties
/// - It supports ad-hoc construction of types within the `TypeSystem`, e.g. `list_of`, `map_of`, ...
pub trait TypeSystem {
    /// Look up a definition by URI.
    fn get(&self, uri: &str) -> Option<DefinitionRef>;

    /// Returns the set of all known URIs.
    fn known_uris(&self) -> HashSet<&str>;

    /// Look up a user-defined type and return it as a [`TypeRef`].
    fn user_defined(&self, uri: &str) -> Result<TypeRef, InvalidTypeError>;

    /// Look up a definition by URI, returning an error if not found.
    fn get_or_err(&self, uri: &str) -> Result<DefinitionRef, InvalidTypeError> {
        self.get(uri)
            .ok_or_else(|| InvalidTypeError::UnknownUri(uri.to_string()))
    }

    /// Creates a list type.
    fn list_of(&self, element: TypeRef) -> TypeRef {
        TypeRef::List(Arc::new(ListType { element }))
    }

    /// Creates a set type.
    fn set_of(&self, element: TypeRef) -> TypeRef {
        TypeRef::Set(Arc::new(SetType { element }))
    }

    /// Creates a map type.
    fn map_of(&self, key: TypeRef, value: TypeRef) -> TypeRef {
        TypeRef::Map(Arc::new(MapType { key, value }))
    }

    /// Resolve a [`TypeId`](type_id::TypeId) to a [`TypeRef`] within this type system.
    fn resolve(&self, type_id: &type_id::TypeId) -> Result<TypeRef, InvalidTypeError> {
        match type_id {
            type_id::TypeId::boolType(_) => Ok(TypeRef::Bool),
            type_id::TypeId::byteType(_) => Ok(TypeRef::Byte),
            type_id::TypeId::i16Type(_) => Ok(TypeRef::I16),
            type_id::TypeId::i32Type(_) => Ok(TypeRef::I32),
            type_id::TypeId::i64Type(_) => Ok(TypeRef::I64),
            type_id::TypeId::floatType(_) => Ok(TypeRef::Float),
            type_id::TypeId::doubleType(_) => Ok(TypeRef::Double),
            type_id::TypeId::stringType(_) => Ok(TypeRef::String),
            type_id::TypeId::binaryType(_) => Ok(TypeRef::Binary),
            type_id::TypeId::anyType(_) => Ok(TypeRef::Any),
            type_id::TypeId::userDefinedType(uri) => self.user_defined(uri),
            type_id::TypeId::listType(list) => {
                let elem = list
                    .elementType
                    .as_ref()
                    .ok_or(InvalidTypeError::EmptyTypeId)?;
                let elem_ref = self.resolve(elem)?;
                Ok(self.list_of(elem_ref))
            }
            type_id::TypeId::setType(set) => {
                let elem = set
                    .elementType
                    .as_ref()
                    .ok_or(InvalidTypeError::EmptyTypeId)?;
                let elem_ref = self.resolve(elem)?;
                Ok(self.set_of(elem_ref))
            }
            type_id::TypeId::mapType(map) => {
                let key = map.keyType.as_ref().ok_or(InvalidTypeError::EmptyTypeId)?;
                let value = map
                    .valueType
                    .as_ref()
                    .ok_or(InvalidTypeError::EmptyTypeId)?;
                let key_ref = self.resolve(key)?;
                let value_ref = self.resolve(value)?;
                Ok(self.map_of(key_ref, value_ref))
            }
            type_id::TypeId::UnknownField(id) => Err(InvalidTypeError::UnresolvableTypeId {
                uri: String::new(),
                detail: format!("unknown TypeId variant (field {id})"),
            }),
        }
    }
}
