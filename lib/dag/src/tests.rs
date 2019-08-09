// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::idmap::IdMap;
use crate::segment::{Dag, Level, Segment};
use drawdag;
use failure::Fallible;
use tempfile::tempdir;

#[test]
fn test_segment_examples() {
    // Examples from segmented-changelog.pdf
    let ascii_page10 = r#"
                C-D-\     /--I--J--\
            A-B------E-F-G-H--------K--L"#;

    let ascii_page11 = r#"
                      T /---------------N--O---\           T
                     / /                        \           \
               /----E-F-\    /-------L--M--------P--\     S--U---\
            A-B-C-D------G--H--I--J--K---------------Q--R---------V--W
                                   \--N"#;

    let ascii_page17 = r#"
              B---D---F--\
            A---C---E-----G"#;

    let ascii_page18 = r#"
             D  C  B
              \  \  \
            A--E--F--G"#;

    let ascii_page19 = r#"
        B---D---F
         \   \   \
      A---C---E---G"#;

    assert_eq!(
        build_segments(ascii_page10, "L", 3, 2).ascii[0],
        r#"
                2-3-\     /--8--9--\
            0-1------4-5-6-7--------10-11
Lv0: 0-1[] 2-3[] 4-7[1, 3] 8-9[6] 10-11[7, 9]
Lv1: 0-7[] 8-11[6, 7]
Lv2: 0-11[]"#
    );

    assert_eq!(
            build_segments(ascii_page11, "W", 3, 3).ascii[0],
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
        build_segments(ascii_page17, "G", 3, 1).ascii[0],
        r#"
              3---4---5--\
            0---1---2-----6
Lv0: 0-2[] 3-5[] 6-6[2, 5]
Lv1: 0-6[]"#
    );

    assert_eq!(
        build_segments(ascii_page18, "G", 3, 3).ascii[0],
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
        build_segments(ascii_page19, "G", 3, 2).ascii[0],
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
        assert_eq!(dag.ancestor(a, b).unwrap(), ancestor);
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
    assert_eq!(dag.ancestor(2, 3).unwrap(), Some(1));
}

// Test utilities

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
        let mut result = String::new();
        let mut last_level = 255;
        let mut segments = self
            .log
            .iter()
            .map(|e| Segment(e.unwrap()))
            .collect::<Vec<_>>();
        segments.sort_by_key(|s| (s.level().unwrap(), s.head().unwrap()));

        for segment in segments {
            let span = segment.span().unwrap();
            let level = segment.level().unwrap();
            if level != last_level {
                if !result.is_empty() {
                    result.push('\n');
                }
                result += &format!("Lv{}: ", level);
                last_level = level;
            } else {
                result.push(' ');
            }
            result += &format!("{}-{}{:?}", span.low, span.high, segment.parents().unwrap());
        }
        result
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
