/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::id::{Group, VertexName};
use crate::ops::DagAddHeads;
use crate::ops::DagPersistent;
use crate::ops::ImportAscii;
use crate::render::render_namedag;
use crate::DagAlgorithm;
use crate::IdMap;
use crate::NameDag;
use crate::NameSet;
use crate::Result;
use crate::SpanSet;
use tempfile::tempdir;
use test_dag::TestDag;

mod test_dag;

#[cfg(test)]
pub mod dummy_dag;

#[cfg(test)]
use crate::iddag::FirstAncestorConstraint;
#[cfg(test)]
use crate::namedag::MemNameDag;
#[cfg(test)]
use crate::ops::IdConvert;
#[cfg(test)]
use crate::protocol::{Process, RequestLocationToName, RequestNameToLocation};
#[cfg(test)]
use crate::Id;

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

fn test_generic_dag1<T: DagAlgorithm + DagAddHeads>(dag: T) -> Result<T> {
    let dag = from_ascii(dag, ASCII_DAG1);
    assert_eq!(expand(dag.all()?), "A B C D E F G H I J K L");
    assert_eq!(expand(dag.ancestors(nameset("H I"))?), "A B C D E F G H I");
    assert_eq!(expand(dag.parents(nameset("H I E"))?), "B D G");
    assert_eq!(expand(dag.children(nameset("G D L"))?), "E H I");
    assert_eq!(expand(dag.roots(nameset("A B E F C D I J"))?), "A C I");
    assert_eq!(expand(dag.heads(nameset("A B E F C D I J"))?), "F J");
    assert_eq!(expand(dag.gca_all(nameset("J K H"))?), "G");
    Ok(dag)
}

fn test_generic_dag_beautify<D: DagAlgorithm + DagAddHeads>(new_dag: impl Fn() -> D) -> Result<()> {
    let ascii = r#"
        A C
        | |
        B D
        |/
        E"#;
    let order = ["B", "D", "A", "C"];
    let dag = from_ascii_with_heads(new_dag(), ascii, Some(&order));
    assert_eq!(expand(dag.all()?), "A B C D E");

    let dag2 = dag.beautify(None)?;
    assert_eq!(expand(dag2.all()?), "A B C D E");

    let dag3 = dag.beautify(Some(nameset("A B E")))?;
    assert_eq!(expand(dag3.all()?), "A B C D E");

    let dag4 = dag.beautify(Some(nameset("C D E")))?;
    assert_eq!(expand(dag4.all()?), "A B C D E");

    let ascii = r#"
        A G
        |/
        B F
        |/
        C E
        |/
        D"#;
    let order = ["C", "E", "G", "F", "A"];
    let dag = from_ascii_with_heads(new_dag(), ascii, Some(&order));
    assert_eq!(expand(dag.all()?), "A B C D E F G");

    let dag2 = dag.beautify(None)?;
    assert_eq!(expand(dag2.all()?), "A B C D E F G");

    let dag3 = dag.beautify(Some(dag.ancestors(nameset("A"))?))?;
    assert_eq!(expand(dag3.all()?), "A B C D E F G");

    let ascii = r#"
        A---B---C---D---E---F---G
             \
              H---I---J---K
                   \
                    L "#;
    let order = ["D", "J", "L", "K", "G"];
    let dag = from_ascii_with_heads(new_dag(), ascii, Some(&order));
    assert_eq!(expand(dag.all()?), "A B C D E F G H I J K L");

    let dag2 = dag.beautify(None)?;
    assert_eq!(expand(dag2.all()?), "A B C D E F G H I J K L");

    Ok(())
}

fn test_generic_dag_reachable_roots(dag: impl DagAlgorithm + DagAddHeads) -> Result<()> {
    let ascii = r#"
         Z
         |\
         D |
         | F
         C |
         | E
         B |
         |/
         A
         "#;
    let dag = from_ascii_with_heads(dag, ascii, Some(&["Z"][..]));

    // B is not reachable without going through other roots (C).
    // A is reachable through Z -> F -> E -> A.
    assert_eq!(
        expand(dag.reachable_roots(nameset("A B C"), nameset("Z"))?),
        "A C"
    );

    // A, E are not reachable without going through other roots (C, F).
    assert_eq!(
        expand(dag.reachable_roots(nameset("A C E F"), nameset("Z"))?),
        "C F"
    );

    // roots and heads overlap.
    assert_eq!(
        expand(dag.reachable_roots(nameset("A B C D E F Z"), nameset("D F"))?),
        "D F"
    );

    // E, F are not reachable.
    assert_eq!(
        expand(dag.reachable_roots(nameset("A B E F"), nameset("D"))?),
        "B"
    );

    // "Bogus" root "Z".
    assert_eq!(
        expand(dag.reachable_roots(nameset("A Z"), nameset("C"))?),
        "A"
    );

    Ok(())
}

fn test_generic_dag_import(dag: impl DagAlgorithm + DagAddHeads) -> Result<()> {
    let ascii = r#"
            J K
           /|\|\
          G H I H
          |/|/|
          E F |
         /|/|\|
        A B C D"#;
    let dag1 = from_ascii_with_heads(dag, ascii, Some(&["J", "K"][..]));

    let dir = tempdir().unwrap();
    let mut dag2 = NameDag::open(&dir.path())?;
    dag2.import_and_flush(&dag1, nameset("J"))?;
    assert_eq!(
        render(&dag2),
        r#"
            K
            ├─╮
            │ │ J
            ╭─┬─┤
            │ I │
            │ ├───╮
            H │ │ │
            ├─────╮
            │ │ │ F
            │ ╭───┼─╮
            │ D │ │ │
            │   │ │ │
            │   │ │ C
            │   │ │
            │   G │
            ├───╯ │
            E     │
            ├─────╮
            │     B
            │
            A"#
    );

    // Check that dag2 is actually flushed to disk.
    let dag3 = NameDag::open(&dir.path())?;
    assert_eq!(
        render(&dag3),
        r#"
            K
            ├─╮
            │ │ J
            ╭─┬─┤
            │ I │
            │ ├───╮
            H │ │ │
            ├─────╮
            │ │ │ F
            │ ╭───┼─╮
            │ D │ │ │
            │   │ │ │
            │   │ │ C
            │   │ │
            │   G │
            ├───╯ │
            E     │
            ├─────╮
            │     B
            │
            A"#
    );
    Ok(())
}

fn test_generic_dag2<T: DagAlgorithm + DagAddHeads>(dag: T) -> Result<T> {
    let ascii = r#"
            J K
           / \|\
          G H I H
          |/|/|
          E F |
         /|/| |
        A B C D"#;
    let dag = from_ascii_with_heads(dag, ascii, Some(&["J", "K"][..]));

    let v = |name: &str| -> VertexName { VertexName::copy_from(name.as_bytes()) };

    assert_eq!(expand(dag.all()?), "A B C D E F G H I J K");
    assert_eq!(expand(dag.ancestors(nameset("H I"))?), "A B C D E F H I");
    assert_eq!(expand(dag.parents(nameset("H I E"))?), "A B D E F");
    assert_eq!(dag.first_ancestor_nth(v("H"), 2)?, v("A"));
    assert_eq!(expand(dag.heads(nameset("E H F K I D"))?), "K");
    assert_eq!(expand(dag.children(nameset("E F I"))?), "G H I J K");
    assert_eq!(expand(dag.roots(nameset("E G H J I K D"))?), "D E");
    assert_eq!(dag.gca_one(nameset("J K"))?, Some(v("I")));
    assert_eq!(expand(dag.gca_all(nameset("J K"))?), "E I");
    assert_eq!(expand(dag.common_ancestors(nameset("G H"))?), "A B E");
    assert!(dag.is_ancestor(v("B"), v("K"))?);
    assert!(!dag.is_ancestor(v("K"), v("B"))?);
    assert_eq!(expand(dag.heads_ancestors(nameset("A E F D G"))?), "D F G");
    assert_eq!(expand(dag.range(nameset("A"), nameset("K"))?), "A E H K");
    assert_eq!(expand(dag.only(nameset("I"), nameset("G"))?), "C D F I");
    let (reachable, unreachable) = dag.only_both(nameset("I"), nameset("G"))?;
    assert_eq!(expand(reachable), "C D F I");
    assert_eq!(expand(unreachable), expand(dag.ancestors(nameset("G"))?));
    assert_eq!(expand(dag.descendants(nameset("F E"))?), "E F G H I J K");

    assert!(dag.is_ancestor(v("B"), v("J"))?);
    assert!(dag.is_ancestor(v("F"), v("F"))?);
    assert!(!dag.is_ancestor(v("K"), v("I"))?);

    Ok(dag)
}

#[test]
fn test_mem_namedag() {
    let dag = test_generic_dag1(MemNameDag::new()).unwrap();
    assert_eq!(
        format!("{:?}", dag),
        r#"Max Level: 1
 Level 1
  Group Master:
   Next Free Id: 12
   Segments: 1
    A+0 : L+11 [] Root
  Group Non-Master:
   Next Free Id: N0
   Segments: 0
 Level 0
  Group Master:
   Next Free Id: 12
   Segments: 12
    L+11 : L+11 [K+10] OnlyHead
    K+10 : K+10 [H+7, J+9] OnlyHead
    J+9 : J+9 [I+8]
    I+8 : I+8 [G+6]
    H+7 : H+7 [G+6] OnlyHead
    G+6 : G+6 [F+5] OnlyHead
    F+5 : F+5 [E+4] OnlyHead
    E+4 : E+4 [B+1, D+3] OnlyHead
    D+3 : D+3 [C+2]
    C+2 : C+2 [] Root
    B+1 : B+1 [A+0] OnlyHead
    A+0 : A+0 [] Root OnlyHead
  Group Non-Master:
   Next Free Id: N0
   Segments: 0
"#
    );
}

#[test]
fn test_dag_reachable_roots() {
    test_generic_dag_reachable_roots(MemNameDag::new()).unwrap()
}

#[test]
fn test_dag_import() {
    test_generic_dag_import(MemNameDag::new()).unwrap()
}

#[test]
fn test_dag_beautify() {
    test_generic_dag_beautify(|| MemNameDag::new()).unwrap()
}

#[test]
fn test_namedag() {
    let dir = tempdir().unwrap();
    let name_dag = NameDag::open(dir.path().join("n")).unwrap();
    let dag = test_generic_dag2(name_dag).unwrap();
    assert_eq!(
        format!("{:?}", dag),
        r#"Max Level: 1
 Level 1
  Group Master:
   Next Free Id: 0
   Segments: 0
  Group Non-Master:
   Next Free Id: N11
   Segments: 2
    H+N9 : K+N10 [E+N2, F+N6, I+N7]
    A+N0 : J+N8 [] Root
 Level 0
  Group Master:
   Next Free Id: 0
   Segments: 0
  Group Non-Master:
   Next Free Id: N11
   Segments: 10
    K+N10 : K+N10 [H+N9, I+N7]
    H+N9 : H+N9 [E+N2, F+N6]
    J+N8 : J+N8 [G+N3, I+N7]
    I+N7 : I+N7 [D+N4, F+N6]
    F+N6 : F+N6 [B+N1, C+N5]
    C+N5 : C+N5 [] Root
    D+N4 : D+N4 [] Root
    E+N2 : G+N3 [A+N0, B+N1]
    B+N1 : B+N1 [] Root
    A+N0 : A+N0 [] Root
"#
    );
}

#[test]
fn test_protocols() {
    let mut built = build_segments(ASCII_DAG1, "A C E L", 3);
    assert_eq!(
        built.ascii[3],
        r#"
                1-3-\     /--8--9--\
            0-2------4-5-6-7--------10-11
Lv0: RH0-0[] R1-1[] 2-2[0] 3-3[1] H4-4[2, 3] H5-7[4] 8-9[6] H10-11[7, 9]
Lv1: R0-0[] R1-1[] 2-4[0, 1] 5-11[4]
Lv2: R0-4[] 5-11[4]
Lv3: R0-11[]"#
    );

    // Replace "[66]" to "B", "[67]" to "C", etc.
    let replace = |mut s: String| -> String {
        for ch in "ABCDEFGHIJKL".chars() {
            s = s.replace(&format!("[{}]", ch as u8), &format!("{}", ch));
        }
        s
    };

    // [Id] -> RequestLocationToName (useful for getting commit hashes from ids).
    let ids: Vec<Id> = (b'A'..=b'L')
        .map(|b| built.name_dag.map.find_id_by_name(&[b]).unwrap().unwrap())
        .collect();
    let request1: RequestLocationToName = (&built.name_dag.map, &built.name_dag.dag)
        .process(ids)
        .unwrap();
    assert_eq!(
        replace(format!("{:?}", &request1)),
        "RequestLocationToName { paths: [B~1, B~0, D~1, D~0, H~3, H~2, H~1, H~0, J~1, J~0, L~1, L~0] }"
    );

    // [name] -> RequestNameToLocation (useful for getting ids from commit hashes).
    let names = (b'A'..=b'L').map(|b| VertexName::copy_from(&[b])).collect();
    let request2: RequestNameToLocation = (&built.name_dag.map, &built.name_dag.dag)
        .process(names)
        .unwrap();
    assert_eq!(
        replace(format!("{:?}", &request2)),
        "RequestNameToLocation { names: [A, B, C, D, E, F, G, H, I, J, K, L], heads: [L] }"
    );

    // RequestLocationToName -> ResponseIdNamePair
    let response1 = (&built.name_dag.map, &built.name_dag.dag)
        .process(request1)
        .unwrap();
    assert_eq!(
        replace(format!("{:?}", &response1)),
        "ResponseIdNamePair { path_names: [(B~1, [A]), (B~0, [B]), (D~1, [C]), (D~0, [D]), (H~3, [E]), (H~2, [F]), (H~1, [G]), (H~0, [H]), (J~1, [I]), (J~0, [J]), (L~1, [K]), (L~0, [L])] }"
    );

    // RequestNameToLocation -> ResponseIdNamePair
    // Only B, D, H, J, L are used since they are "universally known".
    let response2 = (&built.name_dag.map, &built.name_dag.dag)
        .process(request2)
        .unwrap();
    assert_eq!(
        replace(format!("{:?}", &response2)),
        "ResponseIdNamePair { path_names: [(B~1, [A]), (B~0, [B]), (D~1, [C]), (D~0, [D]), (H~3, [E]), (H~2, [F]), (H~1, [G]), (H~0, [H]), (J~1, [I]), (J~0, [J]), (L~1, [K]), (L~0, [L])] }"
    );

    // Applying responses to IdMap. Should not cause errors.
    (&mut built.name_dag.map, &built.name_dag.dag)
        .process(&response1)
        .unwrap();
    (&mut built.name_dag.map, &built.name_dag.dag)
        .process(&response2)
        .unwrap();

    // Try applying response2 to a sparse IdMap.
    // Prepare the sparse IdMap.
    let mut sparse_id_map = IdMap::open(built.dir.path().join("sparse-id")).unwrap();
    built
        .name_dag
        .dag
        .write_sparse_idmap(&built.name_dag.map, &mut sparse_id_map)
        .unwrap();
    assert_eq!(
        format!("{:?}", &sparse_id_map),
        r#"IdMap {
  B: 2,
  D: 3,
  H: 7,
  J: 9,
  L: 11,
}
"#
    );
    // Apply response2.
    (&mut sparse_id_map, &built.name_dag.dag)
        .process(&response2)
        .unwrap();
    assert_eq!(
        format!("{:?}", &sparse_id_map),
        r#"IdMap {
  B: 2,
  D: 3,
  H: 7,
  J: 9,
  L: 11,
  A: 0,
  C: 1,
  E: 4,
  F: 5,
  G: 6,
  I: 8,
  K: 10,
}
"#
    );
}

#[test]
fn test_segment_non_master() {
    let ascii = r#"
a----b----c----d----e----f----g----------h----i
     \                    \             /
      h---i---j---k        l---m---n---o
               \                \
                -----------------p---q"#;
    let built = build_segments(ascii, "i q", 3);
    assert_eq!(
        built.ascii[0],
        r#"
N0---N1---N2---N3---N4---N5---N6---------N11--N12
     \                    \             /
      N11-N12-j---k        N7--N8--N9--N10
               \                \
                -----------------p---q
Lv0: RN0-N6[] N7-N10[N5] N11-N12[N0, N6, N10]
Lv1: RN0-N12[]"#
    );
    assert_eq!(
        built.ascii[1],
        r#"
N0---N1---N2---N3---N4---N5---N6---------N11--N12
     \                    \             /
      N11-N12-N13-k        N7--N8--N9--N10
               \                \
                -----------------N14-N15
Lv0: RN0-N6[] N7-N10[N5] N11-N12[N0, N6, N10] N13-N13[N12] N14-N15[N13, N8]
Lv1: RN0-N12[] N13-N15[N12, N8]
Lv2: RN0-N15[]"#
    );
}

#[test]
fn test_segment_examples() {
    assert_eq!(
        build_segments(ASCII_DAG1, "L", 3).ascii[0],
        r#"
                2-3-\     /--8--9--\
            0-1------4-5-6-7--------10-11
Lv0: RH0-1[] R2-3[] H4-7[1, 3] 8-9[6] H10-11[7, 9]
Lv1: R0-7[] 8-11[6, 7]
Lv2: R0-11[]"#
    );

    assert_eq!(
        build_segments(ASCII_DAG2, "W", 3).ascii[0],
        r#"
                      19/---------------13-14--\           19
                     / /                        \           \
               /----4-5-\    /-------11-12-------15-\     18-20--\
            0-1-2-3------6--7--8--9--10--------------16-17--------21-22
                                   \--13
Lv0: RH0-3[] 4-5[1] H6-10[3, 5] 11-12[7] 13-14[5, 9] 15-15[12, 14] H16-17[10, 15] R18-18[] 19-19[4] 20-20[18, 19] H21-22[17, 20]
Lv1: R0-10[] 11-15[7, 5, 9] 16-17[10, 15] R18-20[4] 21-22[17, 20]
Lv2: R0-17[] R18-22[4, 17]
Lv3: R0-22[]"#
    );

    assert_eq!(
        build_segments(ASCII_DAG3, "G", 3).ascii[0],
        r#"
              3---4---5--\
            0---1---2-----6
Lv0: RH0-2[] R3-5[] H6-6[2, 5]
Lv1: R0-6[]"#
    );

    assert_eq!(
        build_segments(ASCII_DAG4, "G", 3).ascii[0],
        r#"
             3  1  0
              \  \  \
            2--4--5--6
Lv0: RH0-0[] R1-1[] R2-2[] R3-3[] 4-4[2, 3] 5-5[1, 4] H6-6[0, 5]
Lv1: R0-0[] R1-1[] R2-4[] 5-6[1, 4, 0]
Lv2: R0-0[] R1-6[0]
Lv3: R0-6[]"#
    );

    assert_eq!(
        build_segments(ASCII_DAG5, "G", 3).ascii[0],
        r#"
        1---3---5
         \   \   \
      0---2---4---6
Lv0: RH0-0[] R1-1[] H2-2[0, 1] 3-3[1] H4-4[2, 3] 5-5[3] H6-6[4, 5]
Lv1: R0-2[] 3-4[1, 2] 5-6[3, 4]
Lv2: R0-6[]"#
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
    assert_eq!(
        build_segments(ascii_dag, "Y", 3).ascii[0],
        r#"
            0---1--6--11-16
                 \  \  \  \
                  2--7--12-17
                   \  \  \  \
                    3--8--13-18
                     \  \  \  \
                      4--9--14-19
                       \  \  \  \
                        5--10-15-20
Lv0: RH0-5[] 6-6[1] 7-7[6, 2] 8-8[3, 7] 9-9[8, 4] H10-10[5, 9] 11-11[6] 12-12[11, 7] 13-13[12, 8] 14-14[13, 9] H15-15[10, 14] 16-16[11] 17-17[16, 12] 18-18[17, 13] 19-19[14, 18] H20-20[15, 19]
Lv1: R0-5[] 6-8[1, 2, 3] 9-10[8, 4, 5] 11-13[6, 7, 8] 14-15[13, 9, 10] 16-18[11, 12, 13] 19-20[14, 18, 15]
Lv2: R0-10[] 11-15[6, 7, 8, 9, 10] 16-20[11, 12, 13, 14, 15]
Lv3: R0-20[]"#
    );

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
        build_segments(ascii_dag, "G", 3).ascii[0],
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
Lv0: RH0-6[] 7-7[3] 8-10[1] 11-12[7, 10] H13-14[6, 12] 15-17[9] 18-18[11] 19-20[17, 18] H21-21[14, 20]
Lv1: R0-6[] 7-12[3, 1] 13-14[6, 12] 15-20[9, 11] 21-21[14, 20]
Lv2: R0-14[] 15-21[9, 11, 14]
Lv3: R0-21[]"#
    );
}

#[test]
fn test_segment_groups() {
    let dag = r#"
A---B---C---D---E---F---G--------H---I
     \               \          /
      h--i--j--k      l--m--n--o
                \            \
                 -------------p---q"#;

    // This test involves many things. Lower-case commits are non-master commits.
    // - D after B: Test incremental build of a master commit with a master parent.
    // - i after D: Test non-master with master parent.
    // - k after i: Test non-master with non-master parent.
    // - q after G: Test non-master with both master and non-master ancestors.
    // - I after q: Test overwriting non-master Ids with master Ids (!).
    let built = build_segments(dag, "B D i k G q I", 3);
    assert_eq!(
        built.ascii.join("\n"),
        r#"
0---1---C---D---E---F---G--------H---I
     \               \          /
      h--i--j--k      l--m--n--o
                \            \
                 -------------p---q
Lv0: RH0-1[]

0---1---2---3---E---F---G--------H---I
     \               \          /
      h--i--j--k      l--m--n--o
                \            \
                 -------------p---q
Lv0: RH0-1[] H2-3[1]
Lv1: R0-3[]

0---1---2---3---E---F---G--------H---I
     \               \          /
      N0-N1-j--k      l--m--n--o
                \            \
                 -------------p---q
Lv0: RH0-1[] H2-3[1] N0-N1[1]
Lv1: R0-3[] N0-N1[1]

0---1---2---3---E---F---G--------H---I
     \               \          /
      N0-N1-N2-N3     l--m--n--o
                \            \
                 -------------p---q
Lv0: RH0-1[] H2-3[1] N0-N1[1] N2-N3[N1]
Lv1: R0-3[] N0-N3[1]

0---1---2---3---4---5---6--------H---I
     \               \          /
      N0-N1-N2-N3     l--m--n--o
                \            \
                 -------------p---q
Lv0: RH0-1[] H2-3[1] H4-6[3] N0-N1[1] N2-N3[N1]
Lv1: R0-6[] N0-N3[1]

0---1---2---3---4---5---6--------H---I
     \               \          /
      N0-N1-N2-N3     N4-N5-N6-o
                \            \
                 -------------N7--N8
Lv0: RH0-1[] H2-3[1] H4-6[3] N0-N1[1] N2-N3[N1] N4-N6[5] N7-N8[N3, N6]
Lv1: R0-6[] N0-N3[1] N4-N8[5, N3]
Lv2: R0-6[] N0-N8[1, 5]

0---1---2---3---4---5---6--------11--12
     \               \          /
      N0-N1-N2-N3     7--8--9--10
                \            \
                 -------------N4--N5
Lv0: RH0-1[] H2-3[1] H4-6[3] 7-10[5] H11-12[6, 10] N0-N3[1] N4-N5[N3, 9]
Lv1: R0-6[] 7-12[5, 6] N0-N5[1, 9]
Lv2: R0-12[] N0-N5[1, 9]"#
    );

    // Notice that N4 to N6 were re-written in the last step.
    // 'm' only has 1 id: 8 (master). The old id (N5) is now taken by 'q'.
    assert_eq!(
        built.name_dag.map.find_id_by_name(b"m").unwrap().unwrap(),
        Id(8)
    );
    assert_eq!(
        built.name_dag.map.find_name_by_id(Id(8)).unwrap().unwrap(),
        b"m"
    );
    let id = Group::NON_MASTER.min_id() + 5;
    assert_eq!(
        built.name_dag.map.find_name_by_id(id).unwrap().unwrap(),
        b"q"
    );

    // Parent-child indexes work fine.
    assert_eq!(
        format!("{:?}", built.name_dag.dag.children_id(Id(5)).unwrap(),),
        "6 7"
    );
}

#[test]
fn test_namedag_reassign_master() -> crate::Result<()> {
    let dir = tempdir().unwrap();
    let mut dag = NameDag::open(&dir.path())?;
    dag = from_ascii(dag, "A-B-C");

    // The in-memory DAG can answer parent_names questions.
    assert_eq!(format!("{:?}", dag.parent_names("A".into())?), "[]");
    assert_eq!(format!("{:?}", dag.parent_names("C".into())?), "[B]");

    // First flush, A, B, C are non-master.
    dag.flush(&[]).unwrap();

    assert_eq!(format!("{:?}", dag.vertex_id("A".into())?), "N0");
    assert_eq!(format!("{:?}", dag.vertex_id("C".into())?), "N2");

    // Second flush, making B master without adding new vertexes.
    dag.flush(&["B".into()]).unwrap();
    assert_eq!(format!("{:?}", dag.vertex_id("A".into())?), "0");
    assert_eq!(format!("{:?}", dag.vertex_id("B".into())?), "1");
    assert_eq!(format!("{:?}", dag.vertex_id("C".into())?), "N0");

    Ok(())
}

#[test]
fn test_namedag_reassign_non_master() {
    let mut t = TestDag::new();

    // A: master; B, Z: non-master.
    t.drawdag("A--B--Z", &["A"]);
    // C, D, E: non-master.
    t.drawdag("B--C--D--E", &[]);
    // Prompt C to master. Triggers non-master reassignment.
    t.drawdag("", &["C"]);

    // Z still exists.
    assert_eq!(
        t.render_graph(),
        r#"
            Z  N2
            │
            │ E  N1
            │ │
            │ D  N0
            │ │
            │ C  2
            ├─╯
            B  1
            │
            A  0"#
    );

    // Z can round-trip in IdMap.
    let z_id = t.dag.vertex_id("Z".into()).unwrap();
    let z_vertex = t.dag.vertex_name(z_id).unwrap();
    assert_eq!(format!("{:?}", z_vertex), "Z");
}

#[test]
fn test_segment_ancestors_example1() {
    // DAG from segmented-changelog.pdf
    let ascii_dag = r#"
            2-3-\     /--8--9--\
        0-1------4-5-6-7--------10-11"#;
    let result = build_segments(ascii_dag, "11", 3);
    let dag = result.name_dag.dag;

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
        let ancestor = ancestor.map(Id);
        let a = Id(a);
        let b = Id(b);
        assert_eq!(dag.gca_one((a, b)).unwrap(), ancestor);
        assert_eq!(dag.gca_all((a, b)).unwrap().iter().nth(0), ancestor);
        assert_eq!(dag.gca_all((a, b)).unwrap().iter().nth(1), None);
        assert_eq!(dag.is_ancestor(b, a).unwrap(), ancestor == Some(b));
        assert_eq!(dag.is_ancestor(a, b).unwrap(), ancestor == Some(a));
    }

    for (spans, ancestors) in vec![
        (vec![3..=8], vec![3]),
        (vec![1..=1, 4..=9], vec![1]),
        (vec![1..=4], vec![]),
    ] {
        assert_eq!(
            dag.gca_all(SpanSet::from_spans(spans))
                .unwrap()
                .iter()
                .collect::<Vec<Id>>(),
            ancestors.into_iter().map(Id).collect::<Vec<Id>>(),
        );
    }
}

#[test]
fn test_segment_multiple_gcas() {
    let ascii_dag = r#"
        B---C
         \ /
        A---D"#;
    let result = build_segments(ascii_dag, "C D", 3);
    assert_eq!(
        result.ascii[1],
        r#"
        1---2
         \ /
        0---3
Lv0: RH0-0[] R1-1[] H2-2[0, 1] 3-3[0, 1]
Lv1: R0-2[] 3-3[0, 1]"#
    );
    let dag = result.name_dag.dag;
    // This is kind of "undefined" whether it's 1 or 0.
    assert_eq!(dag.gca_one((2, 3)).unwrap(), Some(Id(1)));
    assert_eq!(
        dag.gca_all((2, 3)).unwrap().iter().collect::<Vec<_>>(),
        vec![1, 0]
    );
}

#[test]
fn test_parents() {
    let result = build_segments(ASCII_DAG1, "L", 3);
    assert_eq!(
        result.ascii[0],
        r#"
                2-3-\     /--8--9--\
            0-1------4-5-6-7--------10-11
Lv0: RH0-1[] R2-3[] H4-7[1, 3] 8-9[6] H10-11[7, 9]
Lv1: R0-7[] 8-11[6, 7]
Lv2: R0-11[]"#
    );

    let dag = result.name_dag.dag;

    let parents =
        |spans| -> String { format_set(dag.parents(SpanSet::from_spans(spans)).unwrap()) };
    let parent_ids = |id| -> String { format!("{:?}", dag.parent_ids(Id(id)).unwrap()) };
    let first_ancestor_nth =
        |id, n| -> String { format!("{:?}", dag.first_ancestor_nth(Id(id), n).unwrap()) };
    let to_first_ancestor_nth = |id| -> String {
        let c = FirstAncestorConstraint::KnownUniversally {
            heads: Id(11).into(),
        };
        format!("{:?}", dag.to_first_ancestor_nth(Id(id), c).unwrap())
    };

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

    assert_eq!(parent_ids(0), "[]");
    assert_eq!(parent_ids(1), "[0]");
    assert_eq!(parent_ids(4), "[1, 3]");
    assert_eq!(parent_ids(10), "[7, 9]");
    assert_eq!(parent_ids(11), "[10]");

    assert_eq!(first_ancestor_nth(0, 0), "0");
    assert_eq!(first_ancestor_nth(4, 2), "0");
    assert_eq!(first_ancestor_nth(10, 2), "6");
    assert_eq!(first_ancestor_nth(10, 3), "5");
    assert_eq!(first_ancestor_nth(11, 0), "11");
    assert_eq!(first_ancestor_nth(11, 1), "10");
    assert_eq!(first_ancestor_nth(11, 2), "7");
    assert_eq!(first_ancestor_nth(11, 3), "6");
    assert_eq!(first_ancestor_nth(11, 4), "5");
    assert_eq!(first_ancestor_nth(11, 6), "1");
    assert_eq!(first_ancestor_nth(11, 7), "0");
    assert!(dag.first_ancestor_nth(Id::MIN, 1).is_err());
    assert!(dag.first_ancestor_nth(Id(11), 8).is_err());

    assert_eq!(to_first_ancestor_nth(0), "Some((1, 1))");
    assert_eq!(to_first_ancestor_nth(1), "Some((1, 0))");
    assert_eq!(to_first_ancestor_nth(2), "Some((3, 1))");
    assert_eq!(to_first_ancestor_nth(3), "Some((3, 0))");
    assert_eq!(to_first_ancestor_nth(4), "Some((7, 3))");
    assert_eq!(to_first_ancestor_nth(5), "Some((7, 2))");
    assert_eq!(to_first_ancestor_nth(6), "Some((7, 1))");
    assert_eq!(to_first_ancestor_nth(7), "Some((7, 0))");
    assert_eq!(to_first_ancestor_nth(8), "Some((9, 1))");
    assert_eq!(to_first_ancestor_nth(9), "Some((9, 0))");
    assert_eq!(to_first_ancestor_nth(10), "Some((11, 1))");
    assert_eq!(to_first_ancestor_nth(11), "Some((11, 0))");
}

#[test]
fn test_children() {
    let result = build_segments(ASCII_DAG1, "L", 3);
    let dag = result.name_dag.dag;
    let children =
        |spans| -> String { format_set(dag.children(SpanSet::from_spans(spans)).unwrap()) };

    // See test_parents above for the ASCII DAG.

    assert_eq!(children(vec![]), "");
    assert_eq!(children(vec![0..=0]), "1");

    assert_eq!(children(vec![0..=1]), "1 4");
    assert_eq!(children(vec![0..=2]), "1 3 4");
    assert_eq!(children(vec![0..=3]), "1 3 4");
    assert_eq!(children(vec![0..=4]), "1 3 4 5");
    assert_eq!(children(vec![0..=5]), "1 3..=6");
    assert_eq!(children(vec![0..=6]), "1 3..=8");
    assert_eq!(children(vec![0..=7]), "1 3..=8 10");
    assert_eq!(children(vec![0..=8]), "1 3..=10");
    assert_eq!(children(vec![0..=9]), "1 3..=10");
    assert_eq!(children(vec![0..=10]), "1 3..=11");
    assert_eq!(children(vec![0..=11]), "1 3..=11");

    assert_eq!(children(vec![1..=10]), "3..=11");
    assert_eq!(children(vec![2..=10]), "3..=11");
    assert_eq!(children(vec![3..=10]), "4..=11");
    assert_eq!(children(vec![4..=10]), "5..=11");
    assert_eq!(children(vec![5..=10]), "6..=11");
    assert_eq!(children(vec![6..=10]), "7..=11");
    assert_eq!(children(vec![7..=10]), "9 10 11");
    assert_eq!(children(vec![8..=10]), "9 10 11");
    assert_eq!(children(vec![9..=10]), "10 11");
    assert_eq!(children(vec![10..=10]), "11");

    assert_eq!(children(vec![0..=0, 2..=2]), "1 3");
    assert_eq!(children(vec![0..=0, 3..=3, 5..=5, 9..=10]), "1 4 6 10 11");
    assert_eq!(children(vec![1..=1, 4..=4, 6..=6, 10..=10]), "4 5 7 8 11");
}

#[test]
fn test_heads() {
    let ascii = r#"
    C G   K L
    | |\  |/
    B E F I J
    | |/  |/
    A D   H"#;

    let result = build_segments(ascii, "C G K L J", 2);
    assert_eq!(
        result.ascii[4],
        r#"
    2 6   9 10
    | |\  |/
    1 4 5 8 11
    | |/  |/
    0 3   7
Lv0: RH0-2[] R3-4[] 5-5[3] 6-6[4, 5] R7-9[] 10-10[8] 11-11[7]
Lv1: R0-2[] R3-4[] 5-6[3, 4] R7-9[] 10-10[8] 11-11[7]
Lv2: R0-2[] R3-6[] R7-9[] 10-10[8] 11-11[7]"#
    );

    let dag = result.name_dag.dag;
    let heads = |spans| -> String { format_set(dag.heads(SpanSet::from_spans(spans)).unwrap()) };

    assert_eq!(heads(vec![]), "");
    assert_eq!(heads(vec![0..=11]), "2 6 9 10 11");
    assert_eq!(heads(vec![0..=1, 3..=5, 7..=10]), "1 4 5 9 10");
    assert_eq!(heads(vec![0..=0, 2..=2]), "0 2");
    assert_eq!(heads(vec![1..=2, 4..=6, 7..=7, 11..=11, 9..=9]), "2 6 9 11");
}

#[test]
fn test_roots() {
    let ascii = r#"
    C G   J
    | |\  |\
    B E F I K
    | |/  |\
    A D   H L"#;

    let result = build_segments(ascii, "C G J", 2);
    assert_eq!(
        result.ascii[2],
        r#"
    2 6   11
    | |\  |\
    1 4 5 9 10
    | |/  |\
    0 3   7 8
Lv0: RH0-2[] R3-4[] 5-5[3] 6-6[4, 5] R7-7[] R8-8[] 9-9[7, 8] R10-10[] 11-11[9, 10]
Lv1: R0-2[] R3-4[] 5-6[3, 4] R7-7[] R8-9[7] R10-11[9]
Lv2: R0-2[] R3-6[] R7-9[] R10-11[9]
Lv3: R0-2[] R3-6[] R7-11[]"#
    );

    let dag = result.name_dag.dag;
    let roots = |spans| -> String { format_set(dag.roots(SpanSet::from_spans(spans)).unwrap()) };

    assert_eq!(roots(vec![]), "");
    assert_eq!(roots(vec![0..=11]), "0 3 7 8 10");
    assert_eq!(roots(vec![1..=2, 4..=6, 8..=10]), "1 4 5 8 10");
    assert_eq!(roots(vec![0..=0, 2..=3, 5..=6, 9..=11]), "0 2 3 9 10");
    assert_eq!(roots(vec![1..=1, 3..=3, 6..=8, 11..=11]), "1 3 6 7 8 11");
}

#[test]
fn test_range() {
    let ascii = r#"
            J
           /|\
          G H I
          |/|/
          E F
         /|/|\
        A B C D"#;

    let result = build_segments(ascii, "J", 2);
    assert_eq!(
        result.ascii[0],
        r#"
            9
           /|\
          3 7 8
          |/|/
          2 6
         /|/|\
        0 1 4 5
Lv0: RH0-0[] R1-1[] H2-3[0, 1] R4-4[] R5-5[] 6-6[1, 4, 5] 7-7[2, 6] 8-8[6] H9-9[3, 7, 8]
Lv1: R0-0[] R1-3[0] R4-4[] R5-6[1, 4] 7-7[2, 6] 8-9[6, 3, 7]
Lv2: R0-3[] R4-6[1] 7-9[2, 6, 3]
Lv3: R0-3[] R4-9[1, 2, 3]
Lv4: R0-9[]"#
    );

    let dag = result.name_dag.dag;
    let range = |roots, heads| -> String {
        format_set(
            dag.range(SpanSet::from_spans(roots), SpanSet::from_spans(heads))
                .unwrap(),
        )
    };

    assert_eq!(range(vec![6], vec![3]), "");
    assert_eq!(range(vec![1], vec![3, 8]), "1 2 3 6 8");
    assert_eq!(range(vec![4], vec![3, 8]), "4 6 8");
    assert_eq!(range(vec![0, 5], vec![7]), "0 2 5 6 7");
    assert_eq!(range(vec![0, 5], vec![3, 8]), "0 2 3 5 6 8");
    assert_eq!(range(vec![0, 1, 4, 5], vec![3, 7, 8]), "0..=8");

    assert_eq!(range(vec![0], vec![0]), "0");
    assert_eq!(range(vec![0], vec![1]), "");
    assert_eq!(range(vec![0], vec![2]), "0 2");
    assert_eq!(range(vec![0], vec![3]), "0 2 3");
    assert_eq!(range(vec![0], vec![4]), "");
    assert_eq!(range(vec![0], vec![5]), "");
    assert_eq!(range(vec![0], vec![6]), "");
    assert_eq!(range(vec![0], vec![7]), "0 2 7");
    assert_eq!(range(vec![0], vec![8]), "");
    assert_eq!(range(vec![0], vec![9]), "0 2 3 7 9");
    assert_eq!(range(vec![1], vec![1]), "1");
    assert_eq!(range(vec![1], vec![2]), "1 2");
    assert_eq!(range(vec![1], vec![3]), "1 2 3");
    assert_eq!(range(vec![1], vec![4]), "");
    assert_eq!(range(vec![1], vec![5]), "");
    assert_eq!(range(vec![1], vec![6]), "1 6");
    assert_eq!(range(vec![1], vec![7]), "1 2 6 7");
    assert_eq!(range(vec![1], vec![8]), "1 6 8");
    assert_eq!(range(vec![1], vec![9]), "1 2 3 6..=9");
    assert_eq!(range(vec![2], vec![2]), "2");
    assert_eq!(range(vec![2], vec![3]), "2 3");
    assert_eq!(range(vec![2], vec![4]), "");
    assert_eq!(range(vec![2], vec![5]), "");
    assert_eq!(range(vec![2], vec![6]), "");
    assert_eq!(range(vec![2], vec![7]), "2 7");
    assert_eq!(range(vec![2], vec![8]), "");
    assert_eq!(range(vec![2], vec![9]), "2 3 7 9");
    assert_eq!(range(vec![3], vec![3]), "3");
    assert_eq!(range(vec![3], vec![4]), "");
    assert_eq!(range(vec![3], vec![5]), "");
    assert_eq!(range(vec![3], vec![6]), "");
    assert_eq!(range(vec![3], vec![7]), "");
    assert_eq!(range(vec![3], vec![8]), "");
    assert_eq!(range(vec![3], vec![9]), "3 9");
    assert_eq!(range(vec![4], vec![4]), "4");
    assert_eq!(range(vec![4], vec![5]), "");
    assert_eq!(range(vec![4], vec![6]), "4 6");
    assert_eq!(range(vec![4], vec![7]), "4 6 7");
    assert_eq!(range(vec![4], vec![8]), "4 6 8");
    assert_eq!(range(vec![4], vec![9]), "4 6..=9");
    assert_eq!(range(vec![5], vec![5]), "5");
    assert_eq!(range(vec![5], vec![6]), "5 6");
    assert_eq!(range(vec![5], vec![7]), "5 6 7");
    assert_eq!(range(vec![5], vec![8]), "5 6 8");
    assert_eq!(range(vec![5], vec![9]), "5..=9");
    assert_eq!(range(vec![6], vec![6]), "6");
    assert_eq!(range(vec![6], vec![7]), "6 7");
    assert_eq!(range(vec![6], vec![8]), "6 8");
    assert_eq!(range(vec![6], vec![9]), "6..=9");
    assert_eq!(range(vec![7], vec![7]), "7");
    assert_eq!(range(vec![7], vec![8]), "");
    assert_eq!(range(vec![7], vec![9]), "7 9");
    assert_eq!(range(vec![8], vec![8]), "8");
    assert_eq!(range(vec![8], vec![9]), "8 9");
    assert_eq!(range(vec![9], vec![9]), "9");

    // Test descendants() and ancestors() against range().
    for bits in 0..(1 << 10) {
        let mut set = SpanSet::empty();
        for i in (0..=9).rev() {
            if bits & (1 << i) != 0 {
                set.push_span(i.into());
            }
        }

        let all = dag.all().unwrap();
        assert_eq!(
            dag.range(set.clone(), all.clone()).unwrap().as_spans(),
            dag.descendants(set.clone()).unwrap().as_spans(),
        );

        assert_eq!(
            dag.range(all.clone(), set.clone()).unwrap().as_spans(),
            dag.ancestors(set.clone()).unwrap().as_spans(),
        );
    }
}

// Test utilities

fn expand(set: NameSet) -> String {
    let mut names = set
        .iter()
        .unwrap()
        .map(|n| String::from_utf8_lossy(n.unwrap().as_ref()).to_string())
        .collect::<Vec<String>>();
    names.sort();
    names.join(" ")
}

fn nameset(names: &str) -> NameSet {
    let names: Vec<VertexName> = names
        .split_whitespace()
        .map(|n| VertexName::copy_from(n.as_bytes()))
        .collect();
    NameSet::from_static_names(names)
}

fn format_set(set: SpanSet) -> String {
    format!("{:?}", set)
}

impl IdMap {
    /// Replace names in an ASCII DAG using the ids assigned.
    fn replace(&self, text: &str) -> String {
        let mut result = text.to_string();
        for &group in Group::ALL.iter() {
            for id in group.min_id().to(self.next_free_id(group).unwrap()) {
                if let Ok(Some(name)) = self.find_name_by_id(id) {
                    let name = String::from_utf8(name.to_vec()).unwrap();
                    let id_str = format!("{:01$}", id, name.len());
                    // Try to replace while maintaining width
                    if name.len() + 2 == id_str.len() {
                        result = result
                            .replace(&format!("{}--", name), &id_str)
                            .replace(&format!("{}  ", name), &id_str);
                    } else if name.len() + 1 == id_str.len() {
                        result = result
                            .replace(&format!("{}-", name), &id_str)
                            .replace(&format!("{} ", name), &id_str);
                    }
                    result = result.replace(&format!("{}", name), &id_str);
                }
            }
        }
        result
    }
}

fn get_parents_func_from_ascii(text: &str) -> impl Fn(VertexName) -> Result<Vec<VertexName>> {
    let parents = drawdag::parse(&text);
    move |name: VertexName| -> Result<Vec<VertexName>> {
        Ok(parents[&String::from_utf8(name.as_ref().to_vec()).unwrap()]
            .iter()
            .map(|p| VertexName::copy_from(p.as_bytes()))
            .collect())
    }
}

/// Result of `build_segments`.
pub(crate) struct BuildSegmentResult {
    pub(crate) ascii: Vec<String>,
    pub(crate) name_dag: NameDag,
    pub(crate) dir: tempfile::TempDir,
}

/// Take an ASCII DAG, assign segments from given heads.
/// Return the ASCII DAG and the built NameDag.
pub(crate) fn build_segments(text: &str, heads: &str, segment_size: usize) -> BuildSegmentResult {
    let mut dag = TestDag::new_with_segment_size(segment_size);

    let mut ascii = Vec::new();
    for head in heads.split(' ') {
        // Assign to non-master if the name starts with a lowercase character.
        let master = if head.chars().nth(0).unwrap().is_lowercase() {
            vec![]
        } else {
            vec![head]
        };
        dag.drawdag_with_limited_heads(text, &master[..], Some(&[head]));
        let annotated = dag.annotate_ascii(text);
        let segments = dag.render_segments();
        ascii.push(format!("{}\n{}", annotated, segments));
    }

    BuildSegmentResult {
        ascii,
        name_dag: dag.dag,
        dir: dag.dir,
    }
}

fn from_ascii<D: DagAddHeads>(dag: D, text: &str) -> D {
    from_ascii_with_heads(dag, text, None)
}

fn from_ascii_with_heads<D: DagAddHeads>(mut dag: D, text: &str, heads: Option<&[&str]>) -> D {
    dag.import_ascii_with_heads(text, heads).unwrap();
    dag
}

/// Test a general DAG interface against a few test cases.
pub fn test_generic_dag<D: DagAddHeads + DagAlgorithm + Send + Sync + 'static>(
    new_dag: impl Fn() -> D,
) {
    test_generic_dag1(new_dag()).unwrap();
    test_generic_dag2(new_dag()).unwrap();
    test_generic_dag_reachable_roots(new_dag()).unwrap();
    test_generic_dag_beautify(new_dag).unwrap()
}

fn render(dag: &(impl DagAlgorithm + ?Sized)) -> String {
    render_namedag(dag, |_| None).unwrap()
}
