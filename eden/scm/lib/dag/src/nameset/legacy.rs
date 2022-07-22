/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use super::super::NameDag;
use super::IdStaticSet;
use super::NameSet;
use crate::IdSet;

/// A legacy token that enables conversion between IdSet (id-based)
/// and NameSet (hash-based). It should not be used for new Rust code.
#[derive(Copy, Clone)]
pub struct LegacyCodeNeedIdAccess;

// This is ideally not provided.  However revision numbers in revset still have
// large use-cases in Python and for now we provide this way to convert IdStaticSet
// to IdSet using "revision" numbers.
impl<'a> From<(LegacyCodeNeedIdAccess, &'a IdStaticSet)> for IdSet {
    fn from(value: (LegacyCodeNeedIdAccess, &'a IdStaticSet)) -> IdSet {
        let set = value.1;
        set.spans.clone()
    }
}

impl<'a> From<(LegacyCodeNeedIdAccess, IdSet, &'a NameDag)> for NameSet {
    fn from(value: (LegacyCodeNeedIdAccess, IdSet, &'a NameDag)) -> NameSet {
        NameSet::from_spans_dag(value.1, value.2).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use nonblocking::non_blocking_result as r;

    use super::super::id_static::tests::with_dag;
    use super::*;
    use crate::DagAlgorithm;
    use crate::Result;

    #[test]
    fn test_legacy_convert() -> Result<()> {
        use LegacyCodeNeedIdAccess as L;
        with_dag(|dag| -> Result<()> {
            let set1 = r(dag.ancestors("G".into()))?;
            let spans: IdSet = (
                L,
                set1.as_any().downcast_ref::<IdStaticSet>().unwrap().clone(),
            )
                .into();
            let set2: NameSet = (L, spans.clone(), dag).into();
            assert_eq!(format!("{:?}", &set1), "<spans [E:G+4:6, A:B+0:1]>");
            assert_eq!(format!("{:?}", &set2), "<spans [E:G+4:6, A:B+0:1]>");
            assert_eq!(format!("{:?}", &spans), "0 1 4 5 6");
            Ok(())
        })
    }
}
