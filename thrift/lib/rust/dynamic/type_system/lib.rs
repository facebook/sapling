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

pub mod containers;
pub mod error;
pub mod nodes;
pub mod type_ref;

pub use containers::ListType;
pub use containers::MapType;
pub use containers::SetType;
pub use error::InvalidTypeError;
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
