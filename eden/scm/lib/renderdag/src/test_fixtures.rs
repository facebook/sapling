/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub(crate) struct TestFixture {
    pub(crate) dag: &'static str,
    pub(crate) messages: &'static [(&'static str, &'static str)],
    pub(crate) heads: &'static [&'static str],
    pub(crate) reserve: &'static [&'static str],
    pub(crate) ancestors: &'static [(&'static str, &'static str)],
    pub(crate) missing: &'static [&'static str],
}

pub(crate) const BASIC: TestFixture = TestFixture {
    dag: "A-B-C",
    messages: &[],
    heads: &["C"],
    reserve: &[],
    ancestors: &[],
    missing: &[],
};

pub(crate) const BRANCHES_AND_MERGES: TestFixture = TestFixture {
    dag: r#"
                      T /---------------N--O---\           T
                     / /                        \           \
               /----E-F-\    /-------L--M--------P--\     S--U---\
            A-B-C-D------G--H--I--J--K---------------Q--R---------V--W
                                   \--N
    "#,
    messages: &[],
    heads: &["W"],
    reserve: &[],
    ancestors: &[],
    missing: &[],
};

pub(crate) const OCTOPUS_BRANCH_AND_MERGE: TestFixture = TestFixture {
    dag: r#"
                        /-----\
                       /       \
                      D /--C--\ I
                     / /---D---\ \
                    A-B----E----H-J
                       \---F---/ /
                        \--G--/ F
    "#,
    messages: &[],
    heads: &["J"],
    reserve: &[],
    ancestors: &[],
    missing: &[],
};

pub(crate) const RESERVED_COLUMN: TestFixture = TestFixture {
    dag: r#"
                   A-B-C-F-G----\
                    D-E-/   \-W  \-X-Y-Z
    "#,
    messages: &[],
    heads: &["W", "Z"],
    reserve: &["G"],
    ancestors: &[],
    missing: &[],
};

pub(crate) const ANCESTORS: TestFixture = TestFixture {
    dag: r#"
                   A----B-D-----E----------F-\
                    \-C--/       \-W  \-X     \-Y-Z
    "#,
    messages: &[],
    heads: &["W", "X", "Z"],
    reserve: &["F"],
    ancestors: &[("C", "A"), ("D", "C"), ("E", "D"), ("F", "E")],
    missing: &[],
};

pub(crate) const SPLIT_PARENTS: TestFixture = TestFixture {
    dag: r#"
                    /-B-\     A-\
                   A     D-E  B--E
                    \-C-/     C-/
    "#,
    messages: &[],
    heads: &["E"],
    reserve: &["B", "D", "C"],
    ancestors: &[("E", "A"), ("E", "B")],
    missing: &[],
};

pub(crate) const TERMINATIONS: TestFixture = TestFixture {
    dag: r#"
                   A-B-C  D-E-\
                            F---I--J
                        X-D-H-/  \-K
    "#,
    messages: &[],
    heads: &["C", "J", "K"],
    reserve: &["E"],
    ancestors: &[("B", "A")],
    missing: &["A", "F", "X"],
};

const LONG_MESSAGE: &'static str = "long message 1\nlong message 2\nlong message 3\n\n";
const VERY_LONG_MESSAGE: &'static str =
    "very long message 1\nvery long message 2\nvery long message 3\n\n\
     very long message 4\nvery long message 5\nvery long message 6\n\n";

pub(crate) const LONG_MESSAGES: TestFixture = TestFixture {
    dag: r#"
                         Y-\
                  Z-A-B-D-E-F
                       \-C-/
    "#,
    messages: &[
        ("A", LONG_MESSAGE),
        ("C", LONG_MESSAGE),
        ("F", VERY_LONG_MESSAGE),
    ],
    heads: &["F"],

    reserve: &[],
    ancestors: &[],
    missing: &["Y", "Z"],
};
