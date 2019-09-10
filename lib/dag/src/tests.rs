// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::idmap::IdMap;
use crate::segment::{Dag, Level};
use crate::spanset::SpanSet;
use failure::Fallible;
use tempfile::tempdir;

// Example from segmented-changelog.pdf
// - DAG1: page 10
// - DAG2: page 11
// - DAG3: page 17
// - DAG4: page 18
// - DAG5: page 19

static ASCII_DAG1: &str = r#"
                C-D-\     /--I--J--\
            A-B------E-F-G-H--------K--L"#;

static ASCII_DAG2: &str = r#"
                      T /---------------N--O---\           T
                     / /                        \           \
               /----E-F-\    /-------L--M--------P--\     S--U---\
            A-B-C-D------G--H--I--J--K---------------Q--R---------V--W
                                   \--N"#;

static ASCII_DAG3: &str = r#"
              B---D---F--\
            A---C---E-----G"#;

static ASCII_DAG4: &str = r#"
             D  C  B
              \  \  \
            A--E--F--G"#;

static ASCII_DAG5: &str = r#"
        B---D---F
         \   \   \
      A---C---E---G"#;

#[test]
fn test_segment_examples() {
    assert_eq!(
        build_segments(ASCII_DAG1, "L", 3, 2).ascii[0],
        r#"
                2-3-\     /--8--9--\
            0-1------4-5-6-7--------10-11
Lv0: 0-1[] 2-3[] 4-7[1, 3] 8-9[6] 10-11[7, 9]
Lv1: 0-7[] 8-11[6, 7]
Lv2: 0-11[]"#
    );

    assert_eq!(
            build_segments(ASCII_DAG2, "W", 3, 3).ascii[0],
            r#"
                      19/---------------13-14--\           19
                     / /                        \           \
               /----4-5-\    /-------11-12-------15-\     18-20--\
            0-1-2-3------6--7--8--9--10--------------16-17--------21-22
                                   \--13
Lv0: 0-3[] 4-5[1] 6-10[3, 5] 11-12[7] 13-14[5, 9] 15-15[12, 14] 16-17[10, 15] 18-18[] 19-19[4] 20-20[18, 19] 21-22[17, 20]
Lv1: 0-10[] 11-15[7, 5, 9] 16-17[10, 15] 18-20[4] 21-22[17, 20]
Lv2: 0-17[] 18-22[4, 17]
Lv3: 0-22[]"#
        );

    assert_eq!(
        build_segments(ASCII_DAG3, "G", 3, 1).ascii[0],
        r#"
              3---4---5--\
            0---1---2-----6
Lv0: 0-2[] 3-5[] 6-6[2, 5]
Lv1: 0-6[]"#
    );

    assert_eq!(
        build_segments(ASCII_DAG4, "G", 3, 3).ascii[0],
        r#"
             3  1  0
              \  \  \
            2--4--5--6
Lv0: 0-0[] 1-1[] 2-2[] 3-3[] 4-4[2, 3] 5-5[1, 4] 6-6[0, 5]
Lv1: 0-0[] 1-1[] 2-4[] 5-6[1, 4, 0]
Lv2: 0-0[] 1-6[0]
Lv3: 0-6[]"#
    );

    assert_eq!(
        build_segments(ASCII_DAG5, "G", 3, 2).ascii[0],
        r#"
        1---3---5
         \   \   \
      0---2---4---6
Lv0: 0-0[] 1-1[] 2-2[0, 1] 3-3[1] 4-4[2, 3] 5-5[3] 6-6[4, 5]
Lv1: 0-2[] 3-4[1, 2] 5-6[3, 4]
Lv2: 0-6[]"#
    );

    // Examples outside segmented-changelog.pdf

    // For this graph, the numbers should look continuous in one direction.
    let ascii_dag = r#"
            Z---E--M--J--C
                 \  \  \  \
                  O--T--D--L
                   \  \  \  \
                    K--H--P--W
                     \  \  \  \
                      X--R--U--V
                       \  \  \  \
                        A--N--S--Y"#;
    assert_eq!(build_segments(ascii_dag, "Y", 3, 3).ascii[0], r#"
            0---1--6--11-16
                 \  \  \  \
                  2--7--12-17
                   \  \  \  \
                    3--8--13-18
                     \  \  \  \
                      4--9--14-19
                       \  \  \  \
                        5--10-15-20
Lv0: 0-5[] 6-6[1] 7-7[6, 2] 8-8[3, 7] 9-9[8, 4] 10-10[5, 9] 11-11[6] 12-12[11, 7] 13-13[12, 8] 14-14[13, 9] 15-15[10, 14] 16-16[11] 17-17[16, 12] 18-18[17, 13] 19-19[14, 18] 20-20[15, 19]
Lv1: 0-5[] 6-8[1, 2, 3] 9-10[8, 4, 5] 11-13[6, 7, 8] 14-15[13, 9, 10] 16-18[11, 12, 13] 19-20[14, 18, 15]
Lv2: 0-10[] 11-15[6, 7, 8, 9, 10] 16-20[11, 12, 13, 14, 15]
Lv3: 0-20[]"#);

    // If a graph looks like this, it's hard to optimize anyway.
    let ascii_dag = r#"
            Z---E--J--C--O--T
                 \     \     \
                  D     L     K
                   \     \     \
                    H--P--W--X--R
                     \     \     \
                      U     V     A
                       \     \     \
                        N--S--Y--B--G"#;
    assert_eq!(
        build_segments(ascii_dag, "G", 3, 3).ascii[0],
        r#"
            0---1--2--3--4--5
                 \     \     \
                  8     7     6
                   \     \     \
                    9--10-11-12-13
                     \     \     \
                      15    18    14
                       \     \     \
                        16-17-19-20-21
Lv0: 0-6[] 7-7[3] 8-10[1] 11-12[7, 10] 13-14[6, 12] 15-17[9] 18-18[11] 19-20[17, 18] 21-21[14, 20]
Lv1: 0-6[] 7-12[3, 1] 13-14[6, 12] 15-20[9, 11] 21-21[14, 20]
Lv2: 0-14[] 15-21[9, 11, 14]
Lv3: 0-21[]"#
    );
}

#[test]
fn test_segment_ancestors_example1() {
    // DAG from segmented-changelog.pdf
    let ascii_dag = r#"
            2-3-\     /--8--9--\
        0-1------4-5-6-7--------10-11"#;
    let result = build_segments(ascii_dag, "11", 3, 3);
    let dag = result.dag;

    for (id, count) in vec![
        (11, 12),
        (10, 11),
        (9, 9),
        (8, 8),
        (7, 8),
        (6, 7),
        (5, 6),
        (4, 5),
        (3, 2),
        (2, 1),
        (1, 2),
        (0, 1),
    ] {
        assert_eq!(dag.ancestors(id).unwrap().count(), count);
    }

    for (a, b, ancestor) in vec![
        (10, 3, 3.into()),
        (11, 0, 0.into()),
        (11, 10, 10.into()),
        (11, 9, 9.into()),
        (3, 0, None),
        (7, 1, 1.into()),
        (9, 2, 2.into()),
        (9, 7, 6.into()),
    ] {
        assert_eq!(dag.gca_one((a, b)).unwrap(), ancestor);
        assert_eq!(dag.gca_all((a, b)).unwrap().iter().nth(0), ancestor);
        assert_eq!(dag.gca_all((a, b)).unwrap().iter().nth(1), None);
        assert_eq!(dag.is_ancestor(b, a).unwrap(), ancestor == Some(b));
        assert_eq!(dag.is_ancestor(a, b).unwrap(), ancestor == Some(a));
    }
}

#[test]
fn test_segment_multiple_gcas() {
    let ascii_dag = r#"
        B---C
         \ /
        A---D"#;
    let result = build_segments(ascii_dag, "C D", 3, 1);
    assert_eq!(
        result.ascii[1],
        r#"
        1---2
         \ /
        0---3
Lv0: 0-0[] 1-1[] 2-2[0, 1] 3-3[0, 1]
Lv1: 0-2[] 3-3[0, 1]"#
    );
    let dag = result.dag;
    // This is kind of "undefined" whether it's 1 or 0.
    assert_eq!(dag.gca_one((2, 3)).unwrap(), Some(1));
    assert_eq!(
        dag.gca_all((2, 3)).unwrap().iter().collect::<Vec<_>>(),
        vec![1, 0]
    );
}

#[test]
fn test_parents() {
    let result = build_segments(ASCII_DAG1, "L", 3, 2);
    assert_eq!(
        result.ascii[0],
        r#"
                2-3-\     /--8--9--\
            0-1------4-5-6-7--------10-11
Lv0: 0-1[] 2-3[] 4-7[1, 3] 8-9[6] 10-11[7, 9]
Lv1: 0-7[] 8-11[6, 7]
Lv2: 0-11[]"#
    );

    let dag = result.dag;

    let parents =
        |spans| -> String { format_set(dag.parents(SpanSet::from_spans(spans)).unwrap()) };

    assert_eq!(parents(vec![]), "");

    assert_eq!(parents(vec![0..=0]), "");
    assert_eq!(parents(vec![0..=1]), "0");
    assert_eq!(parents(vec![0..=2]), "0");
    assert_eq!(parents(vec![0..=3]), "0 2");
    assert_eq!(parents(vec![0..=4]), "0..=3");
    assert_eq!(parents(vec![0..=5]), "0..=4");
    assert_eq!(parents(vec![0..=6]), "0..=5");
    assert_eq!(parents(vec![0..=7]), "0..=6");
    assert_eq!(parents(vec![0..=8]), "0..=6");
    assert_eq!(parents(vec![0..=9]), "0..=6 8");
    assert_eq!(parents(vec![0..=10]), "0..=9");
    assert_eq!(parents(vec![0..=11]), "0..=10");

    assert_eq!(parents(vec![0..=0, 2..=2]), "");
    assert_eq!(parents(vec![0..=0, 3..=3, 5..=5, 9..=10]), "2 4 7 8 9");
    assert_eq!(parents(vec![1..=1, 4..=4, 6..=6, 8..=11]), "0 1 3 5..=10");
}

#[test]
fn test_heads() {
    let ascii = r#"
    C G   K L
    | |\  |/
    B E F I J
    | |/  |/
    A D   H"#;

    let result = build_segments(ascii, "C G K L J", 2, 2);
    assert_eq!(
        result.ascii[4],
        r#"
    2 6   9 10
    | |\  |/
    1 4 5 8 11
    | |/  |/
    0 3   7
Lv0: 0-2[] 3-4[] 5-5[3] 6-6[4, 5] 7-9[] 10-10[8] 11-11[7]
Lv1: 0-2[] 3-4[] 5-6[3, 4] 7-9[] 10-10[8] 11-11[7]
Lv2: 0-2[] 3-6[] 7-9[] 10-10[8] 11-11[7]"#
    );

    let dag = result.dag;
    let heads = |spans| -> String { format_set(dag.heads(SpanSet::from_spans(spans)).unwrap()) };

    assert_eq!(heads(vec![]), "");
    assert_eq!(heads(vec![0..=11]), "2 6 9 10 11");
    assert_eq!(heads(vec![0..=1, 3..=5, 7..=10]), "1 4 5 9 10");
    assert_eq!(heads(vec![0..=0, 2..=2]), "0 2");
    assert_eq!(heads(vec![1..=2, 4..=6, 7..=7, 11..=11, 9..=9]), "2 6 9 11");
}

// Test utilities

fn format_set(set: SpanSet) -> String {
    format!("{:?}", set)
}

impl IdMap {
    /// Replace names in an ASCII DAG using the ids assigned.
    fn replace(&self, text: &str) -> String {
        let mut result = text.to_string();
        for id in 0..self.next_free_id() {
            if let Ok(Some(name)) = self.find_slice_by_id(id) {
                let name = String::from_utf8(name.to_vec()).unwrap();
                let id_str = format!("{:01$}", id, name.len());
                if name.len() + 1 == id_str.len() {
                    // Try to replace while maintaining width
                    result = result
                        .replace(&format!("{}-", name), &id_str)
                        .replace(&format!("{} ", name), &id_str);
                }
                result = result.replace(&format!("{}", name), &id_str);
            }
        }
        result
    }
}

impl Dag {
    /// Dump segments in a compact string form.
    fn dump(&self) -> String {
        format!("{:?}", self)
    }
}

/// Result of `build_segments`.
struct BuildSegmentResult {
    ascii: Vec<String>,
    id_map: IdMap,
    dag: Dag,
    dir: tempfile::TempDir,
}

/// Take an ASCII DAG, assign segments from given heads.
/// Return the ASCII DAG and segments strings, together with the IdMap and Dag.
fn build_segments(
    text: &str,
    heads: &str,
    segment_size: usize,
    max_segment_level: Level,
) -> BuildSegmentResult {
    let dir = tempdir().unwrap();
    let mut id_map = IdMap::open(dir.path().join("id")).unwrap();
    let mut dag = Dag::open(dir.path().join("seg")).unwrap();

    let parents = drawdag::parse(&text);
    let parents_by_name = |name: &[u8]| -> Fallible<Vec<Box<[u8]>>> {
        Ok(parents[&String::from_utf8(name.to_vec()).unwrap()]
            .iter()
            .map(|p| p.as_bytes().to_vec().into_boxed_slice())
            .collect())
    };

    let ascii = heads
        .split(' ')
        .map(|head| {
            let head = head.as_bytes();
            id_map.assign_head(head, &parents_by_name).unwrap();
            let head_id = id_map.find_id_by_slice(head).unwrap().unwrap();
            let parents_by_id = id_map.build_get_parents_by_id(&parents_by_name);
            dag.build_flat_segments(head_id, &parents_by_id, 0).unwrap();
            for level in 1..=max_segment_level {
                dag.build_high_level_segments(level, segment_size, false)
                    .unwrap();
            }
            format!("{}\n{}", id_map.replace(text), dag.dump())
        })
        .collect();

    BuildSegmentResult {
        ascii,
        id_map,
        dag,
        dir,
    }
}
