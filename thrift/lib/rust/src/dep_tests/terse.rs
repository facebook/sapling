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

use anyhow::Result;
use fbthrift::builtin_types::OrderedFloat;
use fbthrift::compact_protocol::deserialize;
use fbthrift::compact_protocol::serialize;
use fbthrift_test_if::Double;
use fbthrift_test_if::En;
use fbthrift_test_if::NewInt;
use fbthrift_test_if::TerseEnum;
use fbthrift_test_if::TerseNewType;
use fbthrift_test_if::TerseUnion;
use fbthrift_test_if::Un;
use fbthrift_test_if::UnOne;

#[test]
fn test_terse_enum() {
    let empty = TerseEnum {
        e: En(0),
        ..Default::default()
    };
    assert_eq!(serialize(&empty).as_ref(), &[0x00]);

    let two = TerseEnum {
        e: En::TWO,
        ..Default::default()
    };
    assert_eq!(serialize(&two).as_ref(), &[0x15, 0x04, 0x00]);
}

#[test]
fn test_deprecated_terse_enum() -> Result<()> {
    let zero = test_deprecated_default_enum_min_i32_if::TerseEnum {
        e: test_deprecated_default_enum_min_i32_if::En(0),
        ..Default::default()
    };
    let bytes = serialize(&zero);
    assert_eq!(bytes.as_ref(), &[0x15, 0x00, 0x00]);
    assert_eq!(
        deserialize::<test_deprecated_default_enum_min_i32_if::TerseEnum, _, _>(bytes)?,
        zero
    );
    Ok(())
}

#[test]
fn test_terse_union() {
    assert_eq!(serialize(TerseUnion::default()).as_ref(), &[0x00]);

    let set = TerseUnion {
        u: Un::un1(UnOne::default()),
        ..Default::default()
    };
    assert_eq!(
        serialize(&set).as_ref(),
        &[0x1c, 0x1c, 0x15, 0x00, 0x00, 0x00, 0x00]
    );
}

#[test]
fn test_terse_newtype() {
    assert_eq!(serialize(TerseNewType::default()).as_ref(), &[0x00]);

    let int = TerseNewType {
        i: NewInt(5),
        ..Default::default()
    };
    assert_eq!(serialize(&int).as_ref(), &[0x15, 0x0a, 0x00]);

    let negative_zero = TerseNewType {
        d: Double(OrderedFloat(-0.0)),
        ..Default::default()
    };
    assert_eq!(
        serialize(&negative_zero).as_ref(),
        &[0x27, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
}
