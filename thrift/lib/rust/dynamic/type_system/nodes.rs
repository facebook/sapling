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

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use record::SerializableRecord;

use crate::error::InvalidTypeError;
use crate::type_ref::TypeRef;

/// Structured annotations keyed by annotation URI.
pub type AnnotationsMap = BTreeMap<String, SerializableRecord>;

/// Stable identity of a Thrift field (numeric id and string name).
#[derive(Clone, Debug)]
pub struct FieldIdentity {
    pub id: i16,
    pub name: String,
}

/// How a field's presence is encoded on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresenceQualifier(pub i32);

impl PresenceQualifier {
    pub const UNQUALIFIED: Self = Self(1);
    pub const OPTIONAL: Self = Self(2);
    pub const TERSE: Self = Self(3);
}

/// A field in a struct or union.
#[derive(Clone, Debug)]
pub struct FieldDefinition {
    pub(crate) identity: FieldIdentity,
    pub(crate) presence: PresenceQualifier,
    pub(crate) type_ref: TypeRef,
    pub(crate) custom_default: Option<SerializableRecord>,
    pub(crate) annotations: AnnotationsMap,
}

impl FieldDefinition {
    pub fn identity(&self) -> &FieldIdentity {
        &self.identity
    }

    pub fn presence(&self) -> PresenceQualifier {
        self.presence
    }

    pub fn type_ref(&self) -> &TypeRef {
        &self.type_ref
    }

    pub fn custom_default(&self) -> Option<&SerializableRecord> {
        self.custom_default.as_ref()
    }

    pub fn annotations(&self) -> &AnnotationsMap {
        &self.annotations
    }
}

/// Compact, non-zero handle for O(1) field lookup by position.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FastFieldHandle(u16);

impl FastFieldHandle {
    pub const INVALID: Self = Self(0);

    /// Largest representable field index.
    pub const MAX_INDEX: u16 = u16::MAX - 1;

    pub fn new(index: u16) -> Self {
        Self(
            index
                .checked_add(1)
                .expect("FastFieldHandle index overflow: maximum index is 65534"),
        )
    }

    pub fn is_valid(self) -> bool {
        self.0 != 0
    }

    pub fn index(self) -> u16 {
        assert!(self.is_valid(), "FastFieldHandle::index called on INVALID");
        self.0 - 1
    }
}

/// Common interface for types that contain named fields (structs and unions).
pub trait StructuredNode {
    fn uri(&self) -> &str;
    fn fields(&self) -> &[FieldDefinition];
    fn is_sealed(&self) -> bool;
    fn annotations(&self) -> &AnnotationsMap;

    fn field_by_id(&self, id: i16) -> Option<&FieldDefinition>;
    fn field_by_name(&self, name: &str) -> Option<&FieldDefinition>;
}

/// Definition node for a Thrift struct.
#[derive(Clone, Debug)]
pub struct StructNode {
    pub(crate) uri: String,
    pub(crate) fields: Vec<FieldDefinition>,
    pub(crate) field_index_by_id: HashMap<i16, u16>,
    pub(crate) field_index_by_name: HashMap<String, u16>,
    pub(crate) is_sealed: bool,
    pub(crate) annotations: AnnotationsMap,
}

impl StructNode {
    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn as_type_ref(self: &Arc<Self>) -> TypeRef {
        TypeRef::Struct(Arc::clone(self))
    }
}

impl StructuredNode for StructNode {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn fields(&self) -> &[FieldDefinition] {
        &self.fields
    }

    fn is_sealed(&self) -> bool {
        self.is_sealed
    }

    fn annotations(&self) -> &AnnotationsMap {
        &self.annotations
    }

    fn field_by_id(&self, id: i16) -> Option<&FieldDefinition> {
        self.field_index_by_id
            .get(&id)
            .map(|&idx| &self.fields[idx as usize])
    }

    fn field_by_name(&self, name: &str) -> Option<&FieldDefinition> {
        self.field_index_by_name
            .get(name)
            .map(|&idx| &self.fields[idx as usize])
    }
}

/// Definition node for a Thrift union.
#[derive(Clone, Debug)]
pub struct UnionNode {
    pub(crate) uri: String,
    pub(crate) fields: Vec<FieldDefinition>,
    pub(crate) field_index_by_id: HashMap<i16, u16>,
    pub(crate) field_index_by_name: HashMap<String, u16>,
    pub(crate) is_sealed: bool,
    pub(crate) annotations: AnnotationsMap,
}

impl UnionNode {
    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn as_type_ref(self: &Arc<Self>) -> TypeRef {
        TypeRef::Union(Arc::clone(self))
    }
}

impl StructuredNode for UnionNode {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn fields(&self) -> &[FieldDefinition] {
        &self.fields
    }

    fn is_sealed(&self) -> bool {
        self.is_sealed
    }

    fn annotations(&self) -> &AnnotationsMap {
        &self.annotations
    }

    fn field_by_id(&self, id: i16) -> Option<&FieldDefinition> {
        self.field_index_by_id
            .get(&id)
            .map(|&idx| &self.fields[idx as usize])
    }

    fn field_by_name(&self, name: &str) -> Option<&FieldDefinition> {
        self.field_index_by_name
            .get(name)
            .map(|&idx| &self.fields[idx as usize])
    }
}

/// A single variant within a Thrift enum.
#[derive(Clone, Debug)]
pub struct EnumValue {
    pub(crate) name: String,
    pub(crate) value: i32,
    pub(crate) annotations: AnnotationsMap,
}

impl EnumValue {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    pub fn annotations(&self) -> &AnnotationsMap {
        &self.annotations
    }
}

/// Definition node for a Thrift enum.
#[derive(Clone, Debug)]
pub struct EnumNode {
    pub(crate) uri: String,
    pub(crate) values: Vec<EnumValue>,
    pub(crate) annotations: AnnotationsMap,
}

impl EnumNode {
    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn values(&self) -> &[EnumValue] {
        &self.values
    }

    pub fn annotations(&self) -> &AnnotationsMap {
        &self.annotations
    }

    pub fn as_type_ref(self: &Arc<Self>) -> TypeRef {
        TypeRef::Enum(Arc::clone(self))
    }
}

/// Definition node for a Thrift opaque alias.
#[derive(Clone, Debug)]
pub struct OpaqueAliasNode {
    pub(crate) uri: String,
    pub(crate) target_type: TypeRef,
    pub(crate) annotations: AnnotationsMap,
}

impl OpaqueAliasNode {
    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn target_type(&self) -> &TypeRef {
        &self.target_type
    }

    pub fn annotations(&self) -> &AnnotationsMap {
        &self.annotations
    }

    pub fn as_type_ref(self: &Arc<Self>) -> TypeRef {
        TypeRef::OpaqueAlias(Arc::clone(self))
    }
}

/// Builds id→index and name→index lookup maps, rejecting duplicates.
#[allow(dead_code)] // Called by the builder in the next commit.
pub(crate) fn build_field_indexes(
    uri: &str,
    fields: &[FieldDefinition],
) -> Result<(HashMap<i16, u16>, HashMap<String, u16>), InvalidTypeError> {
    if fields.len() > u16::MAX as usize {
        return Err(InvalidTypeError::TooManyFields(
            fields.len(),
            uri.to_string(),
        ));
    }

    let mut by_id = HashMap::with_capacity(fields.len());
    let mut by_name = HashMap::with_capacity(fields.len());

    for (idx, field) in fields.iter().enumerate() {
        let idx = idx as u16;
        if by_id.insert(field.identity.id, idx).is_some() {
            return Err(InvalidTypeError::DuplicateFieldId(
                field.identity.id,
                uri.to_string(),
            ));
        }
        if by_name.insert(field.identity.name.clone(), idx).is_some() {
            return Err(InvalidTypeError::DuplicateFieldName(
                field.identity.name.clone(),
                uri.to_string(),
            ));
        }
    }

    Ok((by_id, by_name))
}

/// Validates that all fields in a union are optional.
#[allow(dead_code)] // Called by the builder in the next commit.
pub(crate) fn validate_union_fields(
    uri: &str,
    fields: &[FieldDefinition],
) -> Result<(), InvalidTypeError> {
    for field in fields {
        if field.presence != PresenceQualifier::OPTIONAL {
            return Err(InvalidTypeError::NonOptionalUnionField(
                field.identity.id,
                uri.to_string(),
            ));
        }
    }
    Ok(())
}

/// Rejects duplicate enum values or names within a single enum.
#[allow(dead_code)] // Called by the builder in the next commit.
pub(crate) fn validate_enum_values(
    uri: &str,
    values: &[EnumValue],
) -> Result<(), InvalidTypeError> {
    let mut seen_values = HashMap::with_capacity(values.len());
    let mut seen_names = HashMap::with_capacity(values.len());

    for v in values {
        if seen_values.insert(v.value, &v.name).is_some() {
            return Err(InvalidTypeError::DuplicateEnumValue(
                v.value,
                uri.to_string(),
            ));
        }
        if seen_names.insert(&v.name, v.value).is_some() {
            return Err(InvalidTypeError::DuplicateEnumName(
                v.name.clone(),
                uri.to_string(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_field(id: i16, name: &str, presence: PresenceQualifier) -> FieldDefinition {
        FieldDefinition {
            identity: FieldIdentity {
                id,
                name: name.to_string(),
            },
            presence,
            type_ref: TypeRef::Bool,
            custom_default: None,
            annotations: BTreeMap::new(),
        }
    }

    fn make_enum_value(name: &str, value: i32) -> EnumValue {
        EnumValue {
            name: name.to_string(),
            value,
            annotations: BTreeMap::new(),
        }
    }

    #[test]
    fn fast_field_handle_invalid_is_not_valid() {
        assert!(!FastFieldHandle::INVALID.is_valid());
    }

    #[test]
    #[should_panic(expected = "FastFieldHandle::index called on INVALID")]
    fn fast_field_handle_invalid_index_panics() {
        FastFieldHandle::INVALID.index();
    }

    #[test]
    fn fast_field_handle_roundtrip() {
        let h = FastFieldHandle::new(0);
        assert!(h.is_valid());
        assert_eq!(h.index(), 0);

        let h = FastFieldHandle::new(42);
        assert_eq!(h.index(), 42);
    }

    #[test]
    fn fast_field_handle_max_valid_index() {
        let h = FastFieldHandle::new(FastFieldHandle::MAX_INDEX);
        assert!(h.is_valid());
        assert_eq!(h.index(), FastFieldHandle::MAX_INDEX as u16);
    }

    #[test]
    #[should_panic(expected = "FastFieldHandle index overflow")]
    fn fast_field_handle_overflow_panics() {
        FastFieldHandle::new(u16::MAX);
    }

    #[test]
    fn fast_field_handle_new_not_equal_to_invalid() {
        assert_ne!(FastFieldHandle::new(0), FastFieldHandle::INVALID);
    }

    #[test]
    fn structured_node_field_lookup() {
        let fields = vec![
            make_field(1, "x", PresenceQualifier::UNQUALIFIED),
            make_field(2, "y", PresenceQualifier::OPTIONAL),
        ];
        let (by_id, by_name) = build_field_indexes("test::S", &fields).unwrap();
        let node = StructNode {
            uri: "test::S".to_string(),
            fields,
            field_index_by_id: by_id,
            field_index_by_name: by_name,
            is_sealed: false,
            annotations: BTreeMap::new(),
        };

        assert_eq!(node.field_by_id(1).unwrap().identity.id, 1);
        assert_eq!(node.field_by_id(2).unwrap().identity.name, "y");
        assert!(node.field_by_id(99).is_none());

        assert_eq!(node.field_by_name("x").unwrap().identity.id, 1);
        assert_eq!(node.field_by_name("y").unwrap().identity.id, 2);
        assert!(node.field_by_name("z").is_none());
    }

    #[test]
    fn enum_value_accessors() {
        let v = make_enum_value("FOO", 42);
        assert_eq!(v.name(), "FOO");
        assert_eq!(v.value(), 42);
        assert!(v.annotations().is_empty());
    }

    #[test]
    fn build_field_indexes_empty() {
        let (by_id, by_name) = build_field_indexes("test::Empty", &[]).unwrap();
        assert!(by_id.is_empty());
        assert!(by_name.is_empty());
    }

    #[test]
    fn build_field_indexes_success() {
        let fields = vec![
            make_field(1, "foo", PresenceQualifier::UNQUALIFIED),
            make_field(2, "bar", PresenceQualifier::OPTIONAL),
        ];
        let (by_id, by_name) = build_field_indexes("test::S", &fields).unwrap();
        assert_eq!(by_id[&1], 0);
        assert_eq!(by_id[&2], 1);
        assert_eq!(by_name["foo"], 0);
        assert_eq!(by_name["bar"], 1);
    }

    #[test]
    fn build_field_indexes_duplicate_id() {
        let fields = vec![
            make_field(1, "foo", PresenceQualifier::UNQUALIFIED),
            make_field(1, "bar", PresenceQualifier::UNQUALIFIED),
        ];
        let err = build_field_indexes("test::S", &fields).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::DuplicateFieldId(1, ref uri) if uri == "test::S"),
            "expected DuplicateFieldId, got {err:?}"
        );
    }

    #[test]
    fn build_field_indexes_duplicate_name() {
        let fields = vec![
            make_field(1, "foo", PresenceQualifier::UNQUALIFIED),
            make_field(2, "foo", PresenceQualifier::UNQUALIFIED),
        ];
        let err = build_field_indexes("test::S", &fields).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::DuplicateFieldName(ref n, ref uri) if n == "foo" && uri == "test::S"),
            "expected DuplicateFieldName, got {err:?}"
        );
    }

    #[test]
    fn validate_union_fields_all_optional() {
        let fields = vec![
            make_field(1, "a", PresenceQualifier::OPTIONAL),
            make_field(2, "b", PresenceQualifier::OPTIONAL),
        ];
        assert!(validate_union_fields("test::U", &fields).is_ok());
    }

    #[test]
    fn validate_union_fields_empty() {
        assert!(validate_union_fields("test::U", &[]).is_ok());
    }

    #[test]
    fn validate_union_fields_rejects_unqualified() {
        let fields = vec![make_field(1, "a", PresenceQualifier::UNQUALIFIED)];
        let err = validate_union_fields("test::U", &fields).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::NonOptionalUnionField(1, ref uri) if uri == "test::U"),
            "expected NonOptionalUnionField, got {err:?}"
        );
    }

    #[test]
    fn validate_union_fields_rejects_terse() {
        let fields = vec![make_field(1, "a", PresenceQualifier::TERSE)];
        let err = validate_union_fields("test::U", &fields).unwrap_err();
        assert!(matches!(err, InvalidTypeError::NonOptionalUnionField(1, _)));
    }

    #[test]
    fn validate_enum_values_empty() {
        assert!(validate_enum_values("test::E", &[]).is_ok());
    }

    #[test]
    fn validate_enum_values_success() {
        let values = vec![make_enum_value("A", 0), make_enum_value("B", 1)];
        assert!(validate_enum_values("test::E", &values).is_ok());
    }

    #[test]
    fn validate_enum_values_duplicate_value() {
        let values = vec![make_enum_value("A", 0), make_enum_value("B", 0)];
        let err = validate_enum_values("test::E", &values).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::DuplicateEnumValue(0, ref uri) if uri == "test::E"),
            "expected DuplicateEnumValue, got {err:?}"
        );
    }

    #[test]
    fn validate_enum_values_duplicate_name() {
        let values = vec![make_enum_value("A", 0), make_enum_value("A", 1)];
        let err = validate_enum_values("test::E", &values).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::DuplicateEnumName(ref n, ref uri) if n == "A" && uri == "test::E"),
            "expected DuplicateEnumName, got {err:?}"
        );
    }

    #[test]
    fn validate_enum_values_duplicate_both_reports_value_first() {
        let values = vec![make_enum_value("A", 0), make_enum_value("A", 0)];
        let err = validate_enum_values("test::E", &values).unwrap_err();
        assert!(
            matches!(err, InvalidTypeError::DuplicateEnumValue(0, _)),
            "value duplicate should be detected before name duplicate, got {err:?}"
        );
    }
}
