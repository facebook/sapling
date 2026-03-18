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

//! Conversion between `SerializableTypeSystem` and the runtime `TypeSystem`.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use type_system::SerializableEnumDefinition;
use type_system::SerializableFieldDefinition;
use type_system::SerializableOpaqueAliasDefinition;
use type_system::SerializableStructDefinition;
use type_system::SerializableTypeDefinition;
use type_system::SerializableTypeSystem;
use type_system::SerializableUnionDefinition;

use crate::containers::ListType;
use crate::containers::MapType;
use crate::containers::SetType;
use crate::error::InvalidTypeError;
use crate::indexed::DefinitionNode;
use crate::indexed::IndexedTypeSystem;
use crate::nodes::EnumNode;
use crate::nodes::EnumValue;
use crate::nodes::FieldDefinition;
use crate::nodes::FieldIdentity;
use crate::nodes::OpaqueAliasNode;
use crate::nodes::PresenceQualifier;
use crate::nodes::StructNode;
use crate::nodes::UnionNode;
use crate::nodes::build_field_indexes;
use crate::nodes::validate_enum_values;
use crate::nodes::validate_union_fields;
use crate::type_ref::TypeRef;

impl TryFrom<SerializableTypeSystem> for IndexedTypeSystem {
    type Error = InvalidTypeError;

    /// Build a runtime [`IndexedTypeSystem`] from a serializable representation.
    ///
    /// Uses a two-pass approach:
    /// 1. Build a lookup table mapping URIs to placeholder `TypeRef`s
    /// 2. Construct fully-populated nodes using the lookup for cross-references
    ///
    /// Note: Cross-references between types (e.g., struct A referencing struct B)
    /// point to placeholder nodes with empty fields. This is fine for data-level
    /// operations (digest computation, serialization), but Arc pointer identity
    /// is not preserved for cross-references.
    fn try_from(sts: SerializableTypeSystem) -> Result<Self, Self::Error> {
        let types = sts.types;

        if types.is_empty() {
            return Ok(Self::new(HashMap::new()));
        }

        // Pass 1: Create a lookup table with placeholder TypeRefs for each URI.
        // These are used to resolve cross-references during field resolution.
        let mut lookup: HashMap<String, TypeRef> = HashMap::with_capacity(types.len());

        for (uri, entry) in &types {
            if lookup.contains_key(uri.as_str()) {
                return Err(InvalidTypeError::DuplicateUri(uri.clone()));
            }

            let placeholder = match &entry.definition {
                SerializableTypeDefinition::structDef(_) => TypeRef::Struct(Arc::new(StructNode {
                    uri: uri.clone(),
                    fields: Vec::new(),
                    field_index_by_id: HashMap::new(),
                    field_index_by_name: HashMap::new(),
                    is_sealed: false,
                    annotations: BTreeMap::new(),
                })),
                SerializableTypeDefinition::unionDef(_) => TypeRef::Union(Arc::new(UnionNode {
                    uri: uri.clone(),
                    fields: Vec::new(),
                    field_index_by_id: HashMap::new(),
                    field_index_by_name: HashMap::new(),
                    is_sealed: false,
                    annotations: BTreeMap::new(),
                })),
                SerializableTypeDefinition::enumDef(_) => TypeRef::Enum(Arc::new(EnumNode {
                    uri: uri.clone(),
                    values: Vec::new(),
                    annotations: BTreeMap::new(),
                })),
                SerializableTypeDefinition::opaqueAliasDef(_) => {
                    TypeRef::OpaqueAlias(Arc::new(OpaqueAliasNode {
                        uri: uri.clone(),
                        target_type: TypeRef::Bool,
                        annotations: BTreeMap::new(),
                    }))
                }
                SerializableTypeDefinition::UnknownField(id) => {
                    return Err(InvalidTypeError::UnresolvableTypeId {
                        uri: uri.clone(),
                        detail: format!("unknown SerializableTypeDefinition variant (field {id})"),
                    });
                }
            };
            lookup.insert(uri.clone(), placeholder);
        }

        // Pass 2: Build fully-populated nodes, resolving field types via the lookup.
        let mut definitions: HashMap<String, DefinitionNode> = HashMap::with_capacity(types.len());

        for (uri, entry) in &types {
            match &entry.definition {
                SerializableTypeDefinition::structDef(def) => {
                    let fields = resolve_fields(uri, &def.fields, &lookup)?;
                    let (by_id, by_name) = build_field_indexes(uri, &fields)?;
                    definitions.insert(
                        uri.clone(),
                        DefinitionNode::Struct(Arc::new(StructNode {
                            uri: uri.clone(),
                            fields,
                            field_index_by_id: by_id,
                            field_index_by_name: by_name,
                            is_sealed: def.isSealed,
                            annotations: def.annotations.clone(),
                        })),
                    );
                }
                SerializableTypeDefinition::unionDef(def) => {
                    let fields = resolve_fields(uri, &def.fields, &lookup)?;
                    validate_union_fields(uri, &fields)?;
                    let (by_id, by_name) = build_field_indexes(uri, &fields)?;
                    definitions.insert(
                        uri.clone(),
                        DefinitionNode::Union(Arc::new(UnionNode {
                            uri: uri.clone(),
                            fields,
                            field_index_by_id: by_id,
                            field_index_by_name: by_name,
                            is_sealed: def.isSealed,
                            annotations: def.annotations.clone(),
                        })),
                    );
                }
                SerializableTypeDefinition::enumDef(def) => {
                    let values: Vec<EnumValue> = def
                        .values
                        .iter()
                        .map(|v| EnumValue {
                            name: v.name.clone(),
                            value: v.datum,
                            annotations: v.annotations.clone(),
                        })
                        .collect();
                    validate_enum_values(uri, &values)?;
                    definitions.insert(
                        uri.clone(),
                        DefinitionNode::Enum(Arc::new(EnumNode {
                            uri: uri.clone(),
                            values,
                            annotations: def.annotations.clone(),
                        })),
                    );
                }
                SerializableTypeDefinition::opaqueAliasDef(def) => {
                    if matches!(&def.targetType, type_id::TypeId::userDefinedType(_)) {
                        return Err(InvalidTypeError::InvalidOpaqueAlias(uri.clone()));
                    }
                    let target =
                        resolve_type_id_from_lookup(&def.targetType, &lookup).map_err(|e| {
                            InvalidTypeError::UnresolvableTypeId {
                                uri: uri.clone(),
                                detail: format!("opaque alias target: {e}"),
                            }
                        })?;
                    definitions.insert(
                        uri.clone(),
                        DefinitionNode::OpaqueAlias(Arc::new(OpaqueAliasNode {
                            uri: uri.clone(),
                            target_type: target,
                            annotations: def.annotations.clone(),
                        })),
                    );
                }
                SerializableTypeDefinition::UnknownField(_) => unreachable!("handled in pass 1"),
            }
        }

        Ok(Self::new(definitions))
    }
}

/// Convenience wrapper for [`IndexedTypeSystem::try_from`].
pub fn from_serializable(
    sts: SerializableTypeSystem,
) -> Result<IndexedTypeSystem, InvalidTypeError> {
    sts.try_into()
}

impl From<&IndexedTypeSystem> for SerializableTypeSystem {
    fn from(ts: &IndexedTypeSystem) -> Self {
        crate::TypeSystem::to_serializable(ts)
    }
}

/// Convenience wrapper for [`TypeSystem::to_serializable`].
pub fn to_serializable(ts: &impl crate::TypeSystem) -> SerializableTypeSystem {
    ts.to_serializable()
}

// --- Type resolution helpers ---

fn resolve_type_id_from_lookup(
    type_id: &type_id::TypeId,
    lookup: &HashMap<String, TypeRef>,
) -> Result<TypeRef, InvalidTypeError> {
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
        type_id::TypeId::userDefinedType(uri) => lookup
            .get(uri.as_str())
            .cloned()
            .ok_or_else(|| InvalidTypeError::UnknownUri(uri.clone())),
        type_id::TypeId::listType(list) => {
            let elem = list
                .elementType
                .as_ref()
                .ok_or(InvalidTypeError::EmptyTypeId)?;
            let elem_ref = resolve_type_id_from_lookup(elem, lookup)?;
            Ok(TypeRef::List(Arc::new(ListType { element: elem_ref })))
        }
        type_id::TypeId::setType(set) => {
            let elem = set
                .elementType
                .as_ref()
                .ok_or(InvalidTypeError::EmptyTypeId)?;
            let elem_ref = resolve_type_id_from_lookup(elem, lookup)?;
            Ok(TypeRef::Set(Arc::new(SetType { element: elem_ref })))
        }
        type_id::TypeId::mapType(map) => {
            let key = map.keyType.as_ref().ok_or(InvalidTypeError::EmptyTypeId)?;
            let value = map
                .valueType
                .as_ref()
                .ok_or(InvalidTypeError::EmptyTypeId)?;
            let key_ref = resolve_type_id_from_lookup(key, lookup)?;
            let value_ref = resolve_type_id_from_lookup(value, lookup)?;
            Ok(TypeRef::Map(Arc::new(MapType {
                key: key_ref,
                value: value_ref,
            })))
        }
        type_id::TypeId::UnknownField(id) => Err(InvalidTypeError::UnresolvableTypeId {
            uri: std::string::String::new(),
            detail: format!("unknown TypeId variant (field {id})"),
        }),
    }
}

fn resolve_fields(
    uri: &str,
    fields: &[SerializableFieldDefinition],
    lookup: &HashMap<String, TypeRef>,
) -> Result<Vec<FieldDefinition>, InvalidTypeError> {
    fields
        .iter()
        .map(|f| {
            let type_ref = resolve_type_id_from_lookup(&f.r#type, lookup).map_err(|e| {
                InvalidTypeError::UnresolvableTypeId {
                    uri: uri.to_string(),
                    detail: format!("field '{}': {e}", f.identity.name),
                }
            })?;
            Ok(FieldDefinition {
                identity: FieldIdentity {
                    id: f.identity.id,
                    name: f.identity.name.clone(),
                },
                presence: PresenceQualifier(f.presence.0),
                type_ref,
                custom_default: f.customDefaultPartialRecord.clone(),
                annotations: f.annotations.clone(),
            })
        })
        .collect()
}

// --- Serialization helpers (TypeSystem -> SerializableTypeSystem) ---

pub(crate) fn serialize_definition_ref(
    def: &crate::type_ref::DefinitionRef,
) -> SerializableTypeDefinition {
    match def {
        crate::type_ref::DefinitionRef::Struct(node) => {
            SerializableTypeDefinition::structDef(serialize_struct(node))
        }
        crate::type_ref::DefinitionRef::Union(node) => {
            SerializableTypeDefinition::unionDef(serialize_union(node))
        }
        crate::type_ref::DefinitionRef::Enum(node) => {
            SerializableTypeDefinition::enumDef(serialize_enum(node))
        }
        crate::type_ref::DefinitionRef::OpaqueAlias(node) => {
            SerializableTypeDefinition::opaqueAliasDef(serialize_opaque_alias(node))
        }
    }
}

fn serialize_fields(fields: &[FieldDefinition]) -> Vec<SerializableFieldDefinition> {
    fields
        .iter()
        .map(|f| SerializableFieldDefinition {
            identity: type_system::FieldIdentity {
                id: f.identity.id,
                name: f.identity.name.clone(),
                ..Default::default()
            },
            presence: type_system::PresenceQualifier(f.presence.0),
            r#type: f.type_ref.id(),
            customDefaultPartialRecord: f.custom_default.clone(),
            annotations: f.annotations.clone(),
            ..Default::default()
        })
        .collect()
}

fn serialize_struct(node: &StructNode) -> SerializableStructDefinition {
    SerializableStructDefinition {
        fields: serialize_fields(&node.fields),
        isSealed: node.is_sealed,
        annotations: node.annotations.clone(),
        ..Default::default()
    }
}

fn serialize_union(node: &UnionNode) -> SerializableUnionDefinition {
    SerializableUnionDefinition {
        fields: serialize_fields(&node.fields),
        isSealed: node.is_sealed,
        annotations: node.annotations.clone(),
        ..Default::default()
    }
}

fn serialize_enum(node: &EnumNode) -> SerializableEnumDefinition {
    SerializableEnumDefinition {
        values: node
            .values
            .iter()
            .map(|v| type_system::SerializableEnumValueDefinition {
                name: v.name.clone(),
                datum: v.value,
                annotations: v.annotations.clone(),
                ..Default::default()
            })
            .collect(),
        annotations: node.annotations.clone(),
        ..Default::default()
    }
}

fn serialize_opaque_alias(node: &OpaqueAliasNode) -> SerializableOpaqueAliasDefinition {
    SerializableOpaqueAliasDefinition {
        targetType: node.target_type.id(),
        annotations: node.annotations.clone(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use type_id::*;
    use type_system::*;

    use super::*;
    use crate::TypeSystem;
    use crate::nodes::StructuredNode;

    fn i32_type() -> TypeId {
        TypeId::i32Type(I32TypeId::default())
    }

    fn string_type() -> TypeId {
        TypeId::stringType(StringTypeId::default())
    }

    fn make_field(
        id: i16,
        name: &str,
        presence: i32,
        type_id: TypeId,
    ) -> SerializableFieldDefinition {
        SerializableFieldDefinition {
            identity: type_system::FieldIdentity {
                id,
                name: name.to_string(),
                ..Default::default()
            },
            presence: type_system::PresenceQualifier(presence),
            r#type: type_id,
            customDefaultPartialRecord: None,
            annotations: BTreeMap::new(),
            ..Default::default()
        }
    }

    fn make_struct_entry(
        fields: Vec<SerializableFieldDefinition>,
    ) -> SerializableTypeDefinitionEntry {
        SerializableTypeDefinitionEntry {
            definition: SerializableTypeDefinition::structDef(SerializableStructDefinition {
                fields,
                isSealed: false,
                annotations: BTreeMap::new(),
                ..Default::default()
            }),
            sourceInfo: None,
            ..Default::default()
        }
    }

    #[test]
    fn empty_type_system_roundtrip() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::new(),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();
        assert!(ts.known_uris().is_empty());

        let serialized = to_serializable(&ts);
        assert!(serialized.types.is_empty());
    }

    #[test]
    fn single_struct_roundtrip() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/Person".to_string(),
                make_struct_entry(vec![
                    make_field(1, "name", 1, string_type()),
                    make_field(2, "age", 2, i32_type()),
                ]),
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/Person").unwrap();
        assert!(def.is_struct());
        let s = def.as_struct().unwrap();
        assert_eq!(s.fields().len(), 2);
        assert_eq!(s.field_by_id(1).unwrap().identity().name, "name");
        assert_eq!(s.field_by_name("age").unwrap().identity().id, 2);

        let serialized = to_serializable(&ts);
        assert!(serialized.types.contains_key("meta.com/Person"));
    }

    #[test]
    fn enum_roundtrip() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/Status".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::enumDef(SerializableEnumDefinition {
                        values: vec![
                            SerializableEnumValueDefinition {
                                name: "ACTIVE".to_string(),
                                datum: 1,
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                            SerializableEnumValueDefinition {
                                name: "INACTIVE".to_string(),
                                datum: 2,
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                        ],
                        annotations: BTreeMap::new(),
                        ..Default::default()
                    }),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/Status").unwrap();
        assert!(def.is_enum());
        let e = def.as_enum().unwrap();
        assert_eq!(e.values().len(), 2);
        assert_eq!(e.values()[0].name, "ACTIVE");

        let serialized = to_serializable(&ts);
        let entry = &serialized.types["meta.com/Status"];
        match &entry.definition {
            SerializableTypeDefinition::enumDef(ed) => assert_eq!(ed.values.len(), 2),
            _ => panic!("expected enum"),
        }
    }

    #[test]
    fn opaque_alias_roundtrip() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/UserId".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::opaqueAliasDef(
                        SerializableOpaqueAliasDefinition {
                            targetType: TypeId::i64Type(I64TypeId::default()),
                            annotations: BTreeMap::new(),
                            ..Default::default()
                        },
                    ),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/UserId").unwrap();
        assert!(def.is_opaque_alias());
        let oa = def.as_opaque_alias().unwrap();
        assert_eq!(oa.target_type().kind(), crate::type_ref::Kind::I64);
    }

    #[test]
    fn cross_reference_struct() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([
                (
                    "meta.com/A".to_string(),
                    make_struct_entry(vec![make_field(
                        1,
                        "b",
                        1,
                        TypeId::userDefinedType("meta.com/B".to_string()),
                    )]),
                ),
                (
                    "meta.com/B".to_string(),
                    make_struct_entry(vec![make_field(1, "value", 1, i32_type())]),
                ),
            ]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def_a = ts.get("meta.com/A").unwrap();
        let a = def_a.as_struct().unwrap();
        let field_b = a.field_by_name("b").unwrap();
        assert!(field_b.type_ref().is_struct());
        assert_eq!(field_b.type_ref().as_struct().unwrap().uri(), "meta.com/B");
    }

    #[test]
    fn duplicate_uri_error() {
        let entry = make_struct_entry(vec![]);
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([("meta.com/A".to_string(), entry)]),
            ..Default::default()
        };
        // BTreeMap deduplicates, so this won't actually trigger.
        // The error is for the programmatic builder path.
        let ts = from_serializable(sts);
        assert!(ts.is_ok());
    }

    #[test]
    fn opaque_alias_user_defined_target_error() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([
                (
                    "meta.com/Alias".to_string(),
                    SerializableTypeDefinitionEntry {
                        definition: SerializableTypeDefinition::opaqueAliasDef(
                            SerializableOpaqueAliasDefinition {
                                targetType: TypeId::userDefinedType("meta.com/Other".to_string()),
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                        ),
                        sourceInfo: None,
                        ..Default::default()
                    },
                ),
                ("meta.com/Other".to_string(), make_struct_entry(vec![])),
            ]),
            ..Default::default()
        };
        let result = from_serializable(sts);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::InvalidOpaqueAlias(_)),
            "expected InvalidOpaqueAlias, got: {err}",
        );
    }

    // --- Union tests ---

    #[test]
    fn union_roundtrip() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/MyUnion".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::unionDef(SerializableUnionDefinition {
                        fields: vec![
                            make_field(1, "opt1", 2, i32_type()),
                            make_field(2, "opt2", 2, string_type()),
                        ],
                        isSealed: false,
                        annotations: BTreeMap::new(),
                        ..Default::default()
                    }),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/MyUnion").unwrap();
        assert!(def.is_union());
        let u = def.as_union().unwrap();
        assert_eq!(u.fields().len(), 2);
        assert_eq!(u.field_by_id(1).unwrap().identity().name, "opt1");
        assert_eq!(u.field_by_name("opt2").unwrap().identity().id, 2);
    }

    #[test]
    fn union_fields_must_be_optional() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/BadUnion".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::unionDef(SerializableUnionDefinition {
                        fields: vec![make_field(1, "f", 1, i32_type())],
                        isSealed: false,
                        annotations: BTreeMap::new(),
                        ..Default::default()
                    }),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let err = from_serializable(sts).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::NonOptionalUnionField(1, _)),
            "expected NonOptionalUnionField, got: {err}",
        );
    }

    // --- Validation tests ---

    #[test]
    fn duplicate_field_id_error() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![
                    make_field(1, "first", 1, i32_type()),
                    make_field(1, "second", 1, string_type()),
                ]),
            )]),
            ..Default::default()
        };
        let err = from_serializable(sts).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::DuplicateFieldId(1, _)),
            "expected DuplicateFieldId, got: {err}",
        );
    }

    #[test]
    fn duplicate_field_name_error() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![
                    make_field(1, "name", 1, i32_type()),
                    make_field(2, "name", 1, string_type()),
                ]),
            )]),
            ..Default::default()
        };
        let err = from_serializable(sts).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::DuplicateFieldName(ref n, _) if n == "name"),
            "expected DuplicateFieldName, got: {err}",
        );
    }

    #[test]
    fn duplicate_enum_value_error() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/E".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::enumDef(SerializableEnumDefinition {
                        values: vec![
                            SerializableEnumValueDefinition {
                                name: "A".to_string(),
                                datum: 1,
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                            SerializableEnumValueDefinition {
                                name: "B".to_string(),
                                datum: 1,
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                        ],
                        annotations: BTreeMap::new(),
                        ..Default::default()
                    }),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let err = from_serializable(sts).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::DuplicateEnumValue(1, _)),
            "expected DuplicateEnumValue, got: {err}",
        );
    }

    #[test]
    fn duplicate_enum_name_error() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/E".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::enumDef(SerializableEnumDefinition {
                        values: vec![
                            SerializableEnumValueDefinition {
                                name: "A".to_string(),
                                datum: 1,
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                            SerializableEnumValueDefinition {
                                name: "A".to_string(),
                                datum: 2,
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                        ],
                        annotations: BTreeMap::new(),
                        ..Default::default()
                    }),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let err = from_serializable(sts).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::DuplicateEnumName(ref n, _) if n == "A"),
            "expected DuplicateEnumName, got: {err}",
        );
    }

    #[test]
    fn unknown_uri_in_field_type() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![make_field(
                    1,
                    "f",
                    1,
                    TypeId::userDefinedType("meta.com/DoesNotExist".to_string()),
                )]),
            )]),
            ..Default::default()
        };
        let err = from_serializable(sts).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::UnresolvableTypeId { .. }),
            "expected UnresolvableTypeId, got: {err}",
        );
    }

    // --- Container type tests ---

    #[test]
    fn list_field_type() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![make_field(
                    1,
                    "items",
                    1,
                    TypeId::listType(ListTypeId {
                        elementType: Some(Box::new(i32_type())),
                        ..Default::default()
                    }),
                )]),
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/S").unwrap();
        let s = def.as_struct().unwrap();
        let field = s.field_by_name("items").unwrap();
        assert!(field.type_ref().is_list());
        let list = field.type_ref().as_list().unwrap();
        assert!(list.element_type().is_i32());
    }

    #[test]
    fn set_field_type() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![make_field(
                    1,
                    "tags",
                    1,
                    TypeId::setType(SetTypeId {
                        elementType: Some(Box::new(string_type())),
                        ..Default::default()
                    }),
                )]),
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/S").unwrap();
        let s = def.as_struct().unwrap();
        let field = s.field_by_name("tags").unwrap();
        assert!(field.type_ref().is_set());
        let set = field.type_ref().as_set().unwrap();
        assert!(set.element_type().is_string());
    }

    #[test]
    fn map_field_type() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![make_field(
                    1,
                    "props",
                    1,
                    TypeId::mapType(MapTypeId {
                        keyType: Some(Box::new(string_type())),
                        valueType: Some(Box::new(i32_type())),
                        ..Default::default()
                    }),
                )]),
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/S").unwrap();
        let s = def.as_struct().unwrap();
        let field = s.field_by_name("props").unwrap();
        assert!(field.type_ref().is_map());
        let map = field.type_ref().as_map().unwrap();
        assert!(map.key_type().is_string());
        assert!(map.value_type().is_i32());
    }

    #[test]
    fn nested_containers() {
        // list<map<string, i32>>
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![make_field(
                    1,
                    "nested",
                    1,
                    TypeId::listType(ListTypeId {
                        elementType: Some(Box::new(TypeId::mapType(MapTypeId {
                            keyType: Some(Box::new(string_type())),
                            valueType: Some(Box::new(i32_type())),
                            ..Default::default()
                        }))),
                        ..Default::default()
                    }),
                )]),
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/S").unwrap();
        let s = def.as_struct().unwrap();
        let field = s.field_by_name("nested").unwrap();
        assert!(field.type_ref().is_list());
        let inner_map = field.type_ref().as_list().unwrap().element_type();
        assert!(inner_map.is_map());
        assert!(inner_map.as_map().unwrap().key_type().is_string());
        assert!(inner_map.as_map().unwrap().value_type().is_i32());
    }

    #[test]
    fn container_of_user_defined_type() {
        // list<SimpleStruct>
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([
                (
                    "meta.com/Simple".to_string(),
                    make_struct_entry(vec![make_field(1, "value", 1, i32_type())]),
                ),
                (
                    "meta.com/Container".to_string(),
                    make_struct_entry(vec![make_field(
                        1,
                        "items",
                        1,
                        TypeId::listType(ListTypeId {
                            elementType: Some(Box::new(TypeId::userDefinedType(
                                "meta.com/Simple".to_string(),
                            ))),
                            ..Default::default()
                        }),
                    )]),
                ),
            ]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/Container").unwrap();
        let container = def.as_struct().unwrap();
        let items = container.field_by_name("items").unwrap();
        assert!(items.type_ref().is_list());
        let elem = items.type_ref().as_list().unwrap().element_type();
        assert!(elem.is_struct());
        assert_eq!(elem.as_struct().unwrap().uri(), "meta.com/Simple");
    }

    // --- Custom defaults ---

    #[test]
    fn custom_default_field_values() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::structDef(
                        SerializableStructDefinition {
                            fields: vec![
                                SerializableFieldDefinition {
                                    identity: type_system::FieldIdentity {
                                        id: 1,
                                        name: "with_default".to_string(),
                                        ..Default::default()
                                    },
                                    presence: type_system::PresenceQualifier(1),
                                    r#type: i32_type(),
                                    customDefaultPartialRecord: Some(
                                        record::SerializableRecord::int32Datum(42),
                                    ),
                                    annotations: BTreeMap::new(),
                                    ..Default::default()
                                },
                                make_field(2, "without_default", 1, i32_type()),
                            ],
                            isSealed: false,
                            annotations: BTreeMap::new(),
                            ..Default::default()
                        },
                    ),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/S").unwrap();
        let s = def.as_struct().unwrap();
        let with = s.field_by_name("with_default").unwrap();
        assert!(with.custom_default().is_some());
        let without = s.field_by_name("without_default").unwrap();
        assert!(without.custom_default().is_none());
    }

    // --- Annotations ---

    #[test]
    fn struct_annotations() {
        let annotations = BTreeMap::from([(
            "meta.com/MyAnnot".to_string(),
            record::SerializableRecord::boolDatum(true),
        )]);
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::structDef(
                        SerializableStructDefinition {
                            fields: vec![],
                            isSealed: false,
                            annotations,
                            ..Default::default()
                        },
                    ),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/S").unwrap();
        let s = def.as_struct().unwrap();
        assert_eq!(s.annotations().len(), 1);
        assert!(s.annotations().contains_key("meta.com/MyAnnot"));
    }

    // --- Sealed struct ---

    #[test]
    fn sealed_struct() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/Sealed".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::structDef(
                        SerializableStructDefinition {
                            fields: vec![make_field(1, "f", 1, i32_type())],
                            isSealed: true,
                            annotations: BTreeMap::new(),
                            ..Default::default()
                        },
                    ),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/Sealed").unwrap();
        let s = def.as_struct().unwrap();
        assert!(s.is_sealed());
    }

    // --- known_uris ---

    #[test]
    fn known_uris_contains_all_types() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([
                ("meta.com/A".to_string(), make_struct_entry(vec![])),
                ("meta.com/B".to_string(), make_struct_entry(vec![])),
                (
                    "meta.com/C".to_string(),
                    SerializableTypeDefinitionEntry {
                        definition: SerializableTypeDefinition::enumDef(
                            SerializableEnumDefinition {
                                values: vec![SerializableEnumValueDefinition {
                                    name: "X".to_string(),
                                    datum: 1,
                                    annotations: BTreeMap::new(),
                                    ..Default::default()
                                }],
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                        ),
                        sourceInfo: None,
                        ..Default::default()
                    },
                ),
            ]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let uris = ts.known_uris();
        assert_eq!(uris.len(), 3);
        assert!(uris.contains("meta.com/A"));
        assert!(uris.contains("meta.com/B"));
        assert!(uris.contains("meta.com/C"));
    }

    // --- Lookup failures ---

    #[test]
    fn get_unknown_uri_returns_none() {
        let ts = from_serializable(SerializableTypeSystem::default()).unwrap();
        assert!(ts.get("meta.com/DoesNotExist").is_none());
    }

    #[test]
    fn get_or_err_unknown_uri() {
        let ts = from_serializable(SerializableTypeSystem::default()).unwrap();
        let err = ts.get_or_err("meta.com/DoesNotExist").unwrap_err();
        assert!(matches!(err, InvalidTypeError::UnknownUri(_)));
    }

    // --- Negative values ---

    #[test]
    fn negative_field_id() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![make_field(-1, "legacy", 1, i32_type())]),
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/S").unwrap();
        let s = def.as_struct().unwrap();
        assert!(s.field_by_id(-1).is_some());
        assert_eq!(s.field_by_id(-1).unwrap().identity().name, "legacy");
    }

    #[test]
    fn enum_with_negative_values() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/E".to_string(),
                SerializableTypeDefinitionEntry {
                    definition: SerializableTypeDefinition::enumDef(SerializableEnumDefinition {
                        values: vec![
                            SerializableEnumValueDefinition {
                                name: "NEG".to_string(),
                                datum: -1,
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                            SerializableEnumValueDefinition {
                                name: "ZERO".to_string(),
                                datum: 0,
                                annotations: BTreeMap::new(),
                                ..Default::default()
                            },
                        ],
                        annotations: BTreeMap::new(),
                        ..Default::default()
                    }),
                    sourceInfo: None,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();

        let def = ts.get("meta.com/E").unwrap();
        let e = def.as_enum().unwrap();
        assert_eq!(e.values().len(), 2);
        assert_eq!(e.values()[0].value, -1);
    }

    // --- Serializable roundtrip fidelity ---

    #[test]
    fn roundtrip_preserves_field_types() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![
                    make_field(1, "bool_f", 1, TypeId::boolType(BoolTypeId::default())),
                    make_field(2, "i64_f", 2, TypeId::i64Type(I64TypeId::default())),
                    make_field(
                        3,
                        "binary_f",
                        1,
                        TypeId::binaryType(BinaryTypeId::default()),
                    ),
                ]),
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();
        let serialized = to_serializable(&ts);

        let entry = &serialized.types["meta.com/S"];
        match &entry.definition {
            SerializableTypeDefinition::structDef(sd) => {
                assert_eq!(sd.fields.len(), 3);
                assert!(matches!(sd.fields[0].r#type, TypeId::boolType(_)));
                assert!(matches!(sd.fields[1].r#type, TypeId::i64Type(_)));
                assert!(matches!(sd.fields[2].r#type, TypeId::binaryType(_)));
            }
            _ => panic!("expected struct"),
        }
    }

    #[test]
    fn roundtrip_preserves_container_types() {
        let sts = SerializableTypeSystem {
            types: BTreeMap::from([(
                "meta.com/S".to_string(),
                make_struct_entry(vec![make_field(
                    1,
                    "m",
                    1,
                    TypeId::mapType(MapTypeId {
                        keyType: Some(Box::new(string_type())),
                        valueType: Some(Box::new(TypeId::listType(ListTypeId {
                            elementType: Some(Box::new(i32_type())),
                            ..Default::default()
                        }))),
                        ..Default::default()
                    }),
                )]),
            )]),
            ..Default::default()
        };
        let ts = from_serializable(sts).unwrap();
        let serialized = to_serializable(&ts);

        let entry = &serialized.types["meta.com/S"];
        match &entry.definition {
            SerializableTypeDefinition::structDef(sd) => {
                assert!(matches!(sd.fields[0].r#type, TypeId::mapType(_)));
            }
            _ => panic!("expected struct"),
        }
    }
}
