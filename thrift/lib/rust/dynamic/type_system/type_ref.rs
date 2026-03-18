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

use std::sync::Arc;

use crate::containers::ListType;
use crate::containers::MapType;
use crate::containers::SetType;
use crate::error::InvalidTypeError;
use crate::nodes::EnumNode;
use crate::nodes::OpaqueAliasNode;
use crate::nodes::StructNode;
use crate::nodes::StructuredNode;
use crate::nodes::UnionNode;

/// Discriminant for [`TypeRef`] without the associated data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Bool,
    Byte,
    I16,
    I32,
    I64,
    Float,
    Double,
    String,
    Binary,
    Any,
    List,
    Set,
    Map,
    Struct,
    Union,
    Enum,
    OpaqueAlias,
}

/// A reference to a type within a [`TypeSystem`].
///
/// Primitive variants carry no data; user-defined and container variants
/// are shared references into the type graph.
#[derive(Clone, Debug)]
pub enum TypeRef {
    Bool,
    Byte,
    I16,
    I32,
    I64,
    Float,
    Double,
    String,
    Binary,
    Any,
    List(Arc<ListType>),
    Set(Arc<SetType>),
    Map(Arc<MapType>),
    Struct(Arc<StructNode>),
    Union(Arc<UnionNode>),
    Enum(Arc<EnumNode>),
    OpaqueAlias(Arc<OpaqueAliasNode>),
}

impl TypeRef {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Bool => Kind::Bool,
            Self::Byte => Kind::Byte,
            Self::I16 => Kind::I16,
            Self::I32 => Kind::I32,
            Self::I64 => Kind::I64,
            Self::Float => Kind::Float,
            Self::Double => Kind::Double,
            Self::String => Kind::String,
            Self::Binary => Kind::Binary,
            Self::Any => Kind::Any,
            Self::List(_) => Kind::List,
            Self::Set(_) => Kind::Set,
            Self::Map(_) => Kind::Map,
            Self::Struct(_) => Kind::Struct,
            Self::Union(_) => Kind::Union,
            Self::Enum(_) => Kind::Enum,
            Self::OpaqueAlias(_) => Kind::OpaqueAlias,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self.kind() {
            Kind::Bool => "Bool",
            Kind::Byte => "Byte",
            Kind::I16 => "I16",
            Kind::I32 => "I32",
            Kind::I64 => "I64",
            Kind::Float => "Float",
            Kind::Double => "Double",
            Kind::String => "String",
            Kind::Binary => "Binary",
            Kind::Any => "Any",
            Kind::List => "List",
            Kind::Set => "Set",
            Kind::Map => "Map",
            Kind::Struct => "Struct",
            Kind::Union => "Union",
            Kind::Enum => "Enum",
            Kind::OpaqueAlias => "OpaqueAlias",
        }
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool)
    }

    pub fn is_byte(&self) -> bool {
        matches!(self, Self::Byte)
    }

    pub fn is_i16(&self) -> bool {
        matches!(self, Self::I16)
    }

    pub fn is_i32(&self) -> bool {
        matches!(self, Self::I32)
    }

    pub fn is_i64(&self) -> bool {
        matches!(self, Self::I64)
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Self::Float)
    }

    pub fn is_double(&self) -> bool {
        matches!(self, Self::Double)
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Self::String)
    }

    pub fn is_binary(&self) -> bool {
        matches!(self, Self::Binary)
    }

    pub fn is_list(&self) -> bool {
        matches!(self, Self::List(_))
    }

    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    pub fn is_map(&self) -> bool {
        matches!(self, Self::Map(_))
    }

    pub fn is_struct(&self) -> bool {
        matches!(self, Self::Struct(_))
    }

    pub fn is_union(&self) -> bool {
        matches!(self, Self::Union(_))
    }

    pub fn is_enum(&self) -> bool {
        matches!(self, Self::Enum(_))
    }

    pub fn is_opaque_alias(&self) -> bool {
        matches!(self, Self::OpaqueAlias(_))
    }

    pub fn is_structured(&self) -> bool {
        self.is_struct() || self.is_union()
    }

    pub fn as_struct(&self) -> Result<&StructNode, InvalidTypeError> {
        match self {
            Self::Struct(node) => Ok(node),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "Struct",
                actual: self.kind_name(),
            }),
        }
    }

    pub fn as_union(&self) -> Result<&UnionNode, InvalidTypeError> {
        match self {
            Self::Union(node) => Ok(node),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "Union",
                actual: self.kind_name(),
            }),
        }
    }

    pub fn as_enum(&self) -> Result<&EnumNode, InvalidTypeError> {
        match self {
            Self::Enum(node) => Ok(node),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "Enum",
                actual: self.kind_name(),
            }),
        }
    }

    pub fn as_opaque_alias(&self) -> Result<&OpaqueAliasNode, InvalidTypeError> {
        match self {
            Self::OpaqueAlias(node) => Ok(node),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "OpaqueAlias",
                actual: self.kind_name(),
            }),
        }
    }

    pub fn as_structured(&self) -> Result<&dyn StructuredNode, InvalidTypeError> {
        match self {
            Self::Struct(node) => Ok(node.as_ref() as &dyn StructuredNode),
            Self::Union(node) => Ok(node.as_ref() as &dyn StructuredNode),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "Struct or Union",
                actual: self.kind_name(),
            }),
        }
    }

    pub fn as_list(&self) -> Result<&ListType, InvalidTypeError> {
        match self {
            Self::List(t) => Ok(t),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "List",
                actual: self.kind_name(),
            }),
        }
    }

    pub fn as_set(&self) -> Result<&SetType, InvalidTypeError> {
        match self {
            Self::Set(t) => Ok(t),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "Set",
                actual: self.kind_name(),
            }),
        }
    }

    pub fn as_map(&self) -> Result<&MapType, InvalidTypeError> {
        match self {
            Self::Map(t) => Ok(t),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "Map",
                actual: self.kind_name(),
            }),
        }
    }

    /// Widens a user-defined type reference into a general type reference.
    pub fn from_definition(def: &DefinitionRef) -> TypeRef {
        match def {
            DefinitionRef::Struct(n) => TypeRef::Struct(Arc::clone(n)),
            DefinitionRef::Union(n) => TypeRef::Union(Arc::clone(n)),
            DefinitionRef::Enum(n) => TypeRef::Enum(Arc::clone(n)),
            DefinitionRef::OpaqueAlias(n) => TypeRef::OpaqueAlias(Arc::clone(n)),
        }
    }

    /// Returns the serializable [`TypeId`](type_id::TypeId) for this reference.
    pub fn id(&self) -> type_id::TypeId {
        match self {
            Self::Bool => type_id::TypeId::boolType(type_id::BoolTypeId::default()),
            Self::Byte => type_id::TypeId::byteType(type_id::ByteTypeId::default()),
            Self::I16 => type_id::TypeId::i16Type(type_id::I16TypeId::default()),
            Self::I32 => type_id::TypeId::i32Type(type_id::I32TypeId::default()),
            Self::I64 => type_id::TypeId::i64Type(type_id::I64TypeId::default()),
            Self::Float => type_id::TypeId::floatType(type_id::FloatTypeId::default()),
            Self::Double => type_id::TypeId::doubleType(type_id::DoubleTypeId::default()),
            Self::String => type_id::TypeId::stringType(type_id::StringTypeId::default()),
            Self::Binary => type_id::TypeId::binaryType(type_id::BinaryTypeId::default()),
            Self::Any => type_id::TypeId::anyType(type_id::AnyTypeId::default()),
            Self::Struct(node) => type_id::TypeId::userDefinedType(node.uri().to_string()),
            Self::Union(node) => type_id::TypeId::userDefinedType(node.uri().to_string()),
            Self::Enum(node) => type_id::TypeId::userDefinedType(node.uri().to_string()),
            Self::OpaqueAlias(node) => type_id::TypeId::userDefinedType(node.uri().to_string()),
            Self::List(t) => type_id::TypeId::listType(type_id::ListTypeId {
                elementType: Some(Box::new(t.element.id())),
                ..Default::default()
            }),
            Self::Set(t) => type_id::TypeId::setType(type_id::SetTypeId {
                elementType: Some(Box::new(t.element.id())),
                ..Default::default()
            }),
            Self::Map(t) => type_id::TypeId::mapType(type_id::MapTypeId {
                keyType: Some(Box::new(t.key.id())),
                valueType: Some(Box::new(t.value.id())),
                ..Default::default()
            }),
        }
    }
}

/// A reference to a user-defined type definition (struct, union, enum, or opaque
/// alias).
#[derive(Clone, Debug)]
pub enum DefinitionRef {
    Struct(Arc<StructNode>),
    Union(Arc<UnionNode>),
    Enum(Arc<EnumNode>),
    OpaqueAlias(Arc<OpaqueAliasNode>),
}

impl DefinitionRef {
    pub fn uri(&self) -> &str {
        match self {
            Self::Struct(n) => &n.uri,
            Self::Union(n) => &n.uri,
            Self::Enum(n) => &n.uri,
            Self::OpaqueAlias(n) => &n.uri,
        }
    }

    pub fn is_struct(&self) -> bool {
        matches!(self, Self::Struct(_))
    }

    pub fn is_union(&self) -> bool {
        matches!(self, Self::Union(_))
    }

    pub fn is_enum(&self) -> bool {
        matches!(self, Self::Enum(_))
    }

    pub fn is_opaque_alias(&self) -> bool {
        matches!(self, Self::OpaqueAlias(_))
    }

    pub fn is_structured(&self) -> bool {
        self.is_struct() || self.is_union()
    }

    pub fn as_struct(&self) -> Result<&StructNode, InvalidTypeError> {
        match self {
            Self::Struct(n) => Ok(n),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "Struct",
                actual: TypeRef::from_definition(self).kind_name(),
            }),
        }
    }

    pub fn as_union(&self) -> Result<&UnionNode, InvalidTypeError> {
        match self {
            Self::Union(n) => Ok(n),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "Union",
                actual: TypeRef::from_definition(self).kind_name(),
            }),
        }
    }

    pub fn as_enum(&self) -> Result<&EnumNode, InvalidTypeError> {
        match self {
            Self::Enum(n) => Ok(n),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "Enum",
                actual: TypeRef::from_definition(self).kind_name(),
            }),
        }
    }

    pub fn as_opaque_alias(&self) -> Result<&OpaqueAliasNode, InvalidTypeError> {
        match self {
            Self::OpaqueAlias(n) => Ok(n),
            _ => Err(InvalidTypeError::WrongKind {
                expected: "OpaqueAlias",
                actual: TypeRef::from_definition(self).kind_name(),
            }),
        }
    }

    pub fn to_type_ref(&self) -> TypeRef {
        TypeRef::from_definition(self)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::HashMap;

    use super::*;
    use crate::containers::ListType;
    use crate::containers::MapType;
    use crate::containers::SetType;

    fn make_struct_node(uri: &str) -> Arc<StructNode> {
        Arc::new(StructNode {
            uri: uri.to_string(),
            fields: vec![],
            field_index_by_id: HashMap::new(),
            field_index_by_name: HashMap::new(),
            is_sealed: false,
            annotations: BTreeMap::new(),
        })
    }

    fn make_union_node(uri: &str) -> Arc<UnionNode> {
        Arc::new(UnionNode {
            uri: uri.to_string(),
            fields: vec![],
            field_index_by_id: HashMap::new(),
            field_index_by_name: HashMap::new(),
            is_sealed: false,
            annotations: BTreeMap::new(),
        })
    }

    fn make_enum_node(uri: &str) -> Arc<EnumNode> {
        Arc::new(EnumNode {
            uri: uri.to_string(),
            values: vec![],
            annotations: BTreeMap::new(),
        })
    }

    fn make_opaque_alias_node(uri: &str) -> Arc<OpaqueAliasNode> {
        Arc::new(OpaqueAliasNode {
            uri: uri.to_string(),
            target_type: TypeRef::I32,
            annotations: BTreeMap::new(),
        })
    }

    #[test]
    fn kind_returns_correct_variant() {
        assert_eq!(TypeRef::Bool.kind(), Kind::Bool);
        assert_eq!(TypeRef::Byte.kind(), Kind::Byte);
        assert_eq!(TypeRef::I16.kind(), Kind::I16);
        assert_eq!(TypeRef::I32.kind(), Kind::I32);
        assert_eq!(TypeRef::I64.kind(), Kind::I64);
        assert_eq!(TypeRef::Float.kind(), Kind::Float);
        assert_eq!(TypeRef::Double.kind(), Kind::Double);
        assert_eq!(TypeRef::String.kind(), Kind::String);
        assert_eq!(TypeRef::Binary.kind(), Kind::Binary);
        assert_eq!(TypeRef::Any.kind(), Kind::Any);
        assert_eq!(
            TypeRef::List(Arc::new(ListType {
                element: TypeRef::Bool
            }))
            .kind(),
            Kind::List
        );
        assert_eq!(
            TypeRef::Set(Arc::new(SetType {
                element: TypeRef::Bool
            }))
            .kind(),
            Kind::Set
        );
        assert_eq!(
            TypeRef::Map(Arc::new(MapType {
                key: TypeRef::I32,
                value: TypeRef::String
            }))
            .kind(),
            Kind::Map
        );
        assert_eq!(TypeRef::Struct(make_struct_node("S")).kind(), Kind::Struct);
        assert_eq!(TypeRef::Union(make_union_node("U")).kind(), Kind::Union);
        assert_eq!(TypeRef::Enum(make_enum_node("E")).kind(), Kind::Enum);
        assert_eq!(
            TypeRef::OpaqueAlias(make_opaque_alias_node("A")).kind(),
            Kind::OpaqueAlias
        );
    }

    #[test]
    fn kind_name_returns_correct_string() {
        assert_eq!(TypeRef::Bool.kind_name(), "Bool");
        assert_eq!(TypeRef::Float.kind_name(), "Float");
        assert_eq!(TypeRef::Double.kind_name(), "Double");
        assert_eq!(TypeRef::I16.kind_name(), "I16");
        assert_eq!(TypeRef::I32.kind_name(), "I32");
        assert_eq!(TypeRef::I64.kind_name(), "I64");
    }

    #[test]
    fn as_struct_success() {
        let node = make_struct_node("test::S");
        let type_ref = TypeRef::Struct(Arc::clone(&node));
        let result = type_ref.as_struct().unwrap();
        assert_eq!(result.uri(), "test::S");
    }

    #[test]
    fn as_struct_wrong_kind() {
        let err = TypeRef::Bool.as_struct().unwrap_err();
        assert!(matches!(
            err,
            InvalidTypeError::WrongKind {
                expected: "Struct",
                actual: "Bool"
            }
        ));
    }

    #[test]
    fn as_union_success() {
        let node = make_union_node("test::U");
        let type_ref = TypeRef::Union(Arc::clone(&node));
        let result = type_ref.as_union().unwrap();
        assert_eq!(result.uri(), "test::U");
    }

    #[test]
    fn as_union_wrong_kind() {
        let err = TypeRef::I32.as_union().unwrap_err();
        assert!(matches!(
            err,
            InvalidTypeError::WrongKind {
                expected: "Union",
                actual: "I32"
            }
        ));
    }

    #[test]
    fn as_enum_success() {
        let node = make_enum_node("test::E");
        let type_ref = TypeRef::Enum(Arc::clone(&node));
        let result = type_ref.as_enum().unwrap();
        assert_eq!(result.uri(), "test::E");
    }

    #[test]
    fn as_enum_wrong_kind() {
        let err = TypeRef::String.as_enum().unwrap_err();
        assert!(matches!(
            err,
            InvalidTypeError::WrongKind {
                expected: "Enum",
                actual: "String"
            }
        ));
    }

    #[test]
    fn as_opaque_alias_success() {
        let node = make_opaque_alias_node("test::A");
        let type_ref = TypeRef::OpaqueAlias(Arc::clone(&node));
        let result = type_ref.as_opaque_alias().unwrap();
        assert_eq!(result.uri(), "test::A");
    }

    #[test]
    fn as_structured_works_for_struct_and_union() {
        let s = TypeRef::Struct(make_struct_node("S"));
        assert!(s.as_structured().is_ok());

        let u = TypeRef::Union(make_union_node("U"));
        assert!(u.as_structured().is_ok());

        let enum_ref = TypeRef::Enum(make_enum_node("E"));
        let result = enum_ref.as_structured();
        assert!(matches!(
            result,
            Err(InvalidTypeError::WrongKind {
                expected: "Struct or Union",
                actual: "Enum"
            })
        ));
    }

    #[test]
    fn as_list_success() {
        let list = TypeRef::List(Arc::new(ListType {
            element: TypeRef::Bool,
        }));
        let result = list.as_list().unwrap();
        assert_eq!(result.element_type().kind(), Kind::Bool);
    }

    #[test]
    fn as_set_success() {
        let set = TypeRef::Set(Arc::new(SetType {
            element: TypeRef::I64,
        }));
        let result = set.as_set().unwrap();
        assert_eq!(result.element_type().kind(), Kind::I64);
    }

    #[test]
    fn as_map_success() {
        let map = TypeRef::Map(Arc::new(MapType {
            key: TypeRef::String,
            value: TypeRef::I32,
        }));
        let result = map.as_map().unwrap();
        assert_eq!(result.key_type().kind(), Kind::String);
        assert_eq!(result.value_type().kind(), Kind::I32);
    }

    #[test]
    fn from_definition_preserves_arc_identity() {
        let node = make_struct_node("test::S");
        let def = DefinitionRef::Struct(Arc::clone(&node));
        let type_ref = TypeRef::from_definition(&def);
        match type_ref {
            TypeRef::Struct(inner) => assert!(Arc::ptr_eq(&inner, &node)),
            other => panic!("expected TypeRef::Struct, got {other:?}"),
        }
    }

    #[test]
    fn definition_ref_to_type_ref_roundtrip() {
        let node = make_union_node("test::U");
        let def = DefinitionRef::Union(Arc::clone(&node));
        let type_ref = def.to_type_ref();
        assert_eq!(type_ref.kind(), Kind::Union);
    }

    #[test]
    fn definition_ref_uri() {
        let node = make_enum_node("test::E");
        let def = DefinitionRef::Enum(Arc::clone(&node));
        assert_eq!(def.uri(), "test::E");
    }

    #[test]
    fn definition_ref_as_struct_wrong_kind() {
        let def = DefinitionRef::Union(make_union_node("U"));
        let err = def.as_struct().unwrap_err();
        assert!(matches!(
            err,
            InvalidTypeError::WrongKind {
                expected: "Struct",
                actual: "Union"
            }
        ));
    }

    #[test]
    fn is_predicates() {
        assert!(TypeRef::Struct(make_struct_node("S")).is_struct());
        assert!(TypeRef::Union(make_union_node("U")).is_union());
        assert!(TypeRef::Enum(make_enum_node("E")).is_enum());
        assert!(TypeRef::OpaqueAlias(make_opaque_alias_node("A")).is_opaque_alias());

        assert!(TypeRef::Struct(make_struct_node("S")).is_structured());
        assert!(TypeRef::Union(make_union_node("U")).is_structured());
        assert!(!TypeRef::Enum(make_enum_node("E")).is_structured());
        assert!(!TypeRef::Bool.is_structured());
    }
}
