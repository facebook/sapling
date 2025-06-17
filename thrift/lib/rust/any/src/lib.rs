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

mod compression;
mod deserialize;
mod dummy_any;
mod empty_any;
mod serialize;
mod thrift_any_type;
mod uri;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AnyTypeExpectationViolated {
    #[error("`{:?}` and `{:?}` are inconsistent", (.0).0, (.0).1)]
    TypeUrisInconsistent((standard::TypeUri, standard::TypeUri)),
    #[error("`{:?}` and `{:?}` are inconsistent", (.0).0, (.0).1)]
    TypeNamesInconsistent((standard::TypeName, standard::TypeName)),
}

#[derive(Error, Debug)]
pub enum AnyError {
    #[error("Type expectation violation")]
    AnyTypeExpectationViolated(#[from] AnyTypeExpectationViolated),
    #[error("Not a standard protocol `{:?}`", .0)]
    StandardThriftProtocolExpectationViolated(type_rep::ProtocolUnion),
    #[error("Unsupported standard protocol `{:?}`", .0)]
    UnsupportedStandardThriftProtocol(standard::StandardProtocol),
    #[error("Unsupported protocol `{:?}`", .0)]
    UnsupportedThriftProtocol(type_rep::ProtocolUnion),
}

pub use type_::Type as ThriftAnyType;
pub type Any = any::Any;
pub use compression::compress_any;
pub use deserialize::DeserializableFromAny;
pub use deserialize::deserialize;
pub use deserialize::is_type;
pub use dummy_any::GetDummyAny;
pub use empty_any::get_empty_any_struct;
pub use serialize::SerializableThriftObject;
pub use serialize::SerializableToAny;
pub use serialize::serialize;
pub use serialize::serialize_json;
pub use thrift_any_type::GetThriftAnyType;
pub use thrift_any_type::make_thrift_any_type_struct;
pub use thrift_any_type::make_thrift_any_type_union;
pub use uri::get_uri;
