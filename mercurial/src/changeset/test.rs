// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;

use quickcheck::{QuickCheck, TestResult};

use mercurial_types::{Blob, BlobNode, MPath, NodeHash};

use changeset::{escape, unescape, Extra, RevlogChangeset, Time};

const CHANGESET: &[u8] = include_bytes!("cset.bin");
const CHANGESETBLOB: Blob<&[u8]> = Blob::Dirty(CHANGESET);

#[test]
fn test_parse() {
    let csid: NodeHash = "0849d280663e46b3e247857f4a68fabd2ba503c3".parse().unwrap();
    let p1: NodeHash = "169cb9e47f8e86079ee9fd79972092f78fbf68b1".parse().unwrap();
    let node = BlobNode::new(CHANGESETBLOB, Some(&p1), None);
    let cset = RevlogChangeset::parse(node.clone()).expect("parsed");

    assert_eq!(node.nodeid().expect("no nodeid"), csid);

    assert_eq!(
        cset,
        RevlogChangeset {
            parents: *node.parents(),
            manifestid: "497522ef3706a1665bf4140497c65b467454e962".parse().unwrap(),
            user: "Mads Kiilerich <madski@unity3d.com>".into(),
            time: Time {
                time: 1383910550,
                tz: -3600,
            },
            extra: Extra(
                vec![("branch".into(), "stable".into())]
                    .into_iter()
                    .collect()
            ),
            files: vec![MPath::new(b"mercurial/util.py").unwrap()],
            comments: r#"util: warn when adding paths ending with \

Paths ending with \ will fail the verification introduced in 684a977c2ae0 when
checking out on Windows ... and if it didn't fail it would probably not do what
the user expected."#.into(),
        }
    );
}

#[test]
fn test_generate() {
    let csid: NodeHash = "0849d280663e46b3e247857f4a68fabd2ba503c3".parse().unwrap();
    let p1: NodeHash = "169cb9e47f8e86079ee9fd79972092f78fbf68b1".parse().unwrap();
    let node = BlobNode::new(CHANGESETBLOB, Some(&p1), None);
    let cset = RevlogChangeset::parse(node.clone()).expect("parsed");

    assert_eq!(node.nodeid().expect("no nodeid"), csid);

    let mut new = Vec::new();

    cset.generate(&mut new).expect("generate failed");

    assert_eq!(new, CHANGESET);
}

quickcheck! {
    fn escape_roundtrip(s: Vec<u8>) -> bool {
        let esc = escape(&s);
        let unesc = unescape(&esc);
        if s != unesc {
            println!("s: {:?}, esc: {:?}, unesc: {:?}", s, esc, unesc)
        }
        s == unesc
    }
}

fn extras_roundtrip_prop(kv: BTreeMap<Vec<u8>, Vec<u8>>) -> TestResult {
    if kv.keys().any(|k| k.contains(&b':')) {
        return TestResult::discard();
    }

    let extra = Extra(kv);
    let mut enc = Vec::new();
    let () = extra.generate(&mut enc).expect("enc failed");
    let new = Extra::from_slice(Some(&enc)).expect("parse failed");

    TestResult::from_bool(new == extra)
}

#[test]
fn extras_roundtrip() {
    QuickCheck::new()
        .tests(50)  // more takes too much time
        .quickcheck(extras_roundtrip_prop as fn(BTreeMap<Vec<u8>, Vec<u8>>) -> TestResult);
}

#[test]
#[ignore]
fn extras_roundtrip_long() {
    QuickCheck::new().tests(1000).quickcheck(
        extras_roundtrip_prop as fn(BTreeMap<Vec<u8>, Vec<u8>>) -> TestResult,
    );
}
