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
use fbthrift::compact_protocol::deserialize;
use fbthrift::compact_protocol::serialize;

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
