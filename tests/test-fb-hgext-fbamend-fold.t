Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=
  > inhibit=
  > rebase=
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF
  $ reset() {
  >   cd ..
  >   rm -rf repo
  >   hg init repo
  >   cd repo
  > }

Set up repo.
  $ hg init repo && cd repo
  $ hg debugbuilddag -m "+5 *4 +2"
  $ showgraph
  o  7 r7
  |
  o  6 r6
  |
  o  5 r5
  |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  |/
  o  1 r1
  |
  o  0 r0

Test that a fold works correctly on error.
  $ hg fold --exact 7 7
  single revision specified, nothing to fold
  [1]

Test simple case of folding a head. Should work normally.
  $ hg up 7
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg fold --from '.^'
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  8 r6
  |
  o  5 r5
  |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  |/
  o  1 r1
  |
  o  0 r0

Test rebasing of stack after fold.
  $ hg up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg fold --from '.^'
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing 4:b762560d23fd "r4"
  merging mf
  $ showgraph
  o  10 r4
  |
  @  9 r2
  |
  | o  8 r6
  | |
  | o  5 r5
  |/
  o  1 r1
  |
  o  0 r0

Test rebasing of multiple children
  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg fold --from '.^'
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing 5:* "r5" (glob)
  merging mf
  rebasing 8:* "r6" (glob)
  merging mf
  rebasing 9:* "r2" (glob)
  merging mf
  rebasing 10:* "r4" (glob)
  merging mf
  $ showgraph
  o  15 r4
  |
  o  14 r2
  |
  | o  13 r6
  | |
  | o  12 r5
  |/
  @  11 r0

Test folding multiple changesets, using default behavior of folding
up to working copy parent. Also tests situation where the branch to
rebase is not on the topmost folded commit.
  $ reset
  $ hg debugbuilddag -m "+5 *4 +2"
  $ showgraph
  o  7 r7
  |
  o  6 r6
  |
  o  5 r5
  |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  |/
  o  1 r1
  |
  o  0 r0

  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg fold --from 2
  3 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing 3:a422badec216 "r3"
  merging mf
  rebasing 4:b762560d23fd "r4"
  merging mf
  rebasing 5:77d787dfa5b6 "r5"
  merging mf
  rebasing 6:ec6d8e65acbe "r6"
  merging mf
  rebasing 7:9c9414e0356c "r7"
  merging mf
  $ showgraph
  o  13 r7
  |
  o  12 r6
  |
  o  11 r5
  |
  | o  10 r4
  | |
  | o  9 r3
  |/
  @  8 r0

Test folding changesets unrelated to working copy parent using --exact.
Also test that using node hashes instead of rev numbers works.
  $ reset
  $ hg debugbuilddag -m +6
  $ showgraph
  o  5 r5
  |
  o  4 r4
  |
  o  3 r3
  |
  o  2 r2
  |
  o  1 r1
  |
  o  0 r0

  $ hg fold --exact 09bb8c f07e66 cb14eb
  3 changesets folded
  rebasing 4:aa70f0fe546a "r4"
  merging mf
  rebasing 5:f2987ebe5838 "r5"
  merging mf
  $ showgraph
  o  8 r5
  |
  o  7 r4
  |
  o  6 r1
  |
  o  0 r0

Test --no-rebase flag.
  $ hg fold --no-rebase --exact 6 7
  2 changesets folded
  $ showgraph
  o  9 r1
  |
  | o  8 r5
  | |
  | x  7 r4
  | |
  | x  6 r1
  |/
  o  0 r0

Test that bookmarks are correctly moved.
  $ reset
  $ hg debugbuilddag +3
  $ hg bookmarks -r 1 test1
  $ hg bookmarks -r 2 test2_1
  $ hg bookmarks -r 2 test2_2
  $ hg bookmarks
     test1                     1:* (glob)
     test2_1                   2:* (glob)
     test2_2                   2:* (glob)
  $ hg fold --exact 1 2
  2 changesets folded
  $ hg bookmarks
     test1                     3:* (glob)
     test2_1                   3:* (glob)
     test2_2                   3:* (glob)

Test JSON output
  $ reset
  $ hg debugbuilddag -m +6
  $ hg up 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  5 r5
  |
  o  4 r4
  |
  o  3 r3
  |
  o  2 r2
  |
  o  1 r1
  |
  o  0 r0

When rebase is not involved
  $ hg fold --from -r '.^' -Tjson -q
  [
   {
    "count": 2,
    "nodechanges": {"aa70f0fe546a3536a4b9d49297099d140203494f": ["329a7569e12e1828787ecfebc262b012abcf7077"], "f2987ebe583896be81f8361000878a6f4b30e53a": ["329a7569e12e1828787ecfebc262b012abcf7077"]}
   }
  ]

  $ hg fold --from -r '.^' -T '{nodechanges|json}' -q
  {"329a7569e12e1828787ecfebc262b012abcf7077": ["befa2830d024c4b14c1d5331052d7a13ec2df124"], "cb14eba0ad9cc49472e54fe97c261f5f78a79dab": ["befa2830d024c4b14c1d5331052d7a13ec2df124"]} (no-eol)

  $ showgraph
  @  7 r3
  |
  o  2 r2
  |
  o  1 r1
  |
  o  0 r0

XXX: maybe we also want the rebase nodechanges here.
When rebase is involved
  $ hg fold --exact 1 2 -Tjson -q
  [
   {
    "count": 2,
    "nodechanges": {"09bb8c08de89bca9fffcd6ed3530d6178f07d9e2": ["d65bf110c68ee2cf0a0ba076da90df3fcf76229b"], "f07e66f449d06b214d0a8a9b1a6fa8af2f5f79a5": ["d65bf110c68ee2cf0a0ba076da90df3fcf76229b"]}
   }
  ]

  $ hg fold --exact 0 8 -T '{nodechanges|json}' -q
  {"d65bf110c68ee2cf0a0ba076da90df3fcf76229b": ["785c10c9aad58fba814a235f074a79bdc5535083"], "fdaccbb26270c9a42503babe11fd846d7300df0b": ["785c10c9aad58fba814a235f074a79bdc5535083"]} (no-eol)
