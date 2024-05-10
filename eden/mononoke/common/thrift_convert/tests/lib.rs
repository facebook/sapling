/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thrift_convert::ThriftConvert;
use thrift_convert_test as thrift;

#[derive(ThriftConvert, Debug, Eq, PartialEq, Clone)]
#[thrift(thrift::ThriftStruct)]
struct TestStruct {
    a: u32,
    b: String,
    c: i64,
    d: Vec<i32>,
    e: SubTestStruct,
    f: Vec<SubTestStruct>,
}

#[derive(ThriftConvert, Debug, Eq, PartialEq, Clone)]
#[thrift(thrift::ThriftSecondStruct)]
struct SubTestStruct {
    x: u64,
    y: String,
}

#[derive(ThriftConvert, Debug, Eq, PartialEq, Clone)]
#[thrift(thrift::ThriftUnion)]
enum TestEnum {
    #[thrift(thrift::ThriftEmpty)]
    First,
    Second(TestStruct),
    Third(SubTestStruct),
}

fn round_trips(s: impl ThriftConvert + std::fmt::Debug + Clone + Eq) {
    let thrift = s.clone().into_thrift();
    let round_triped = ThriftConvert::from_thrift(thrift).unwrap();
    assert_eq!(s, round_triped);
}

#[test]
fn test_derive_thrift_convert() {
    let test = TestStruct {
        a: 1,
        b: "hello".to_string(),
        c: -2,
        d: vec![10, 20, 30],
        e: SubTestStruct {
            x: 3,
            y: "olleh".to_string(),
        },
        f: vec![
            SubTestStruct {
                x: 1,
                y: "world".to_string(),
            },
            SubTestStruct {
                x: 2,
                y: "dlrow".to_string(),
            },
        ],
    };

    let test_enum_first = TestEnum::First;
    round_trips(test_enum_first);

    let test_enum_second = TestEnum::Second(test.clone());
    round_trips(test_enum_second);

    let test_enum_third = TestEnum::Third(test.e.clone());
    round_trips(test_enum_third);

    round_trips(test);
}
