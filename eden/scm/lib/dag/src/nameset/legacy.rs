/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::super::NameDag;
use super::super::SpanSet;
use super::DagSet;
use super::NameSet;

/// A legacy token that enables conversion between SpanSet (id-based)
/// and NameSet (hash-based). It should not be used for new Rust code.
#[derive(Copy, Clone)]
pub struct LegacyCodeNeedIdAccess;

// This is ideally not provided.  However revision numbers in revset still have
// large use-cases in Python and for now we provide this way to convert DagSet
// to SpanSet using "revision" numbers.
impl<'a> From<(LegacyCodeNeedIdAccess, &'a DagSet)> for SpanSet {
    fn from(value: (LegacyCodeNeedIdAccess, &'a DagSet)) -> SpanSet {
        let set = value.1;
        set.spans.clone()
    }
}

impl<'a> From<(LegacyCodeNeedIdAccess, SpanSet, &'a NameDag)> for NameSet {
    fn from(value: (LegacyCodeNeedIdAccess, SpanSet, &'a NameDag)) -> NameSet {
        NameSet::from_spans_idmap(value.1, value.2.snapshot_map.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::super::dag::tests::with_dag;
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_legacy_convert() -> Result<()> {
        use LegacyCodeNeedIdAccess as L;
        with_dag(|dag| -> Result<()> {
            let set1 = dag.ancestors("G".into())?;
            let spans: SpanSet =
                (L, set1.as_any().downcast_ref::<DagSet>().unwrap().clone()).into();
            let set2: NameSet = (L, spans.clone(), dag).into();
            assert_eq!(format!("{:?}", &set1), "<dag [0 1 4 5 6]>");
            assert_eq!(format!("{:?}", &set2), "<dag [0 1 4 5 6]>");
            assert_eq!(format!("{:?}", &spans), "0 1 4 5 6");
            Ok(())
        })
    }
}
