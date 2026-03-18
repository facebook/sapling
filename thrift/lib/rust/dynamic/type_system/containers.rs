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

use crate::type_ref::TypeRef;

/// Thrift `list<T>` type descriptor.
#[derive(Clone, Debug)]
pub struct ListType {
    pub(crate) element: TypeRef,
}

impl ListType {
    pub fn element_type(&self) -> &TypeRef {
        &self.element
    }
}

/// Thrift `set<T>` type descriptor.
#[derive(Clone, Debug)]
pub struct SetType {
    pub(crate) element: TypeRef,
}

impl SetType {
    pub fn element_type(&self) -> &TypeRef {
        &self.element
    }
}

/// Thrift `map<K, V>` type descriptor.
#[derive(Clone, Debug)]
pub struct MapType {
    pub(crate) key: TypeRef,
    pub(crate) value: TypeRef,
}

impl MapType {
    pub fn key_type(&self) -> &TypeRef {
        &self.key
    }

    pub fn value_type(&self) -> &TypeRef {
        &self.value
    }
}
