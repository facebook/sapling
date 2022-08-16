#chg-compatible
#debugruntest-compatible

Set up test environment.
  $ configure mutation-norecord
  $ enable amend rebase
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
  o  9c9414e0356c r7
  │
  o  ec6d8e65acbe r6
  │
  o  77d787dfa5b6 r5
  │
  │ o  b762560d23fd r4
  │ │
  │ o  a422badec216 r3
  │ │
  │ o  37d4c1cec295 r2
  ├─╯
  o  f177fbb9e8d1 r1
  │
  o  93cbaf5e6529 r0

Test that a fold works correctly on error.
  $ hg fold --exact 'desc(r7)' 'desc(r7)'
  single revision specified, nothing to fold
  [1]

Test simple case of folding a head. Should work normally.
  $ hg up 'desc(r7)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg fold --from '.^'
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  dd0541003d21 r6
  │
  o  77d787dfa5b6 r5
  │
  │ o  b762560d23fd r4
  │ │
  │ o  a422badec216 r3
  │ │
  │ o  37d4c1cec295 r2
  ├─╯
  o  f177fbb9e8d1 r1
  │
  o  93cbaf5e6529 r0

Test rebasing of stack after fold.
  $ hg up 'desc(r3)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg fold --from '.^'
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing b762560d23fd "r4"
  merging mf
  $ showgraph
  o  c480ccdc36c0 r4
  │
  @  fac8d040c80b r2
  │
  │ o  dd0541003d21 r6
  │ │
  │ o  77d787dfa5b6 r5
  ├─╯
  o  f177fbb9e8d1 r1
  │
  o  93cbaf5e6529 r0

Test rebasing of multiple children
  $ hg up 'desc(r1)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg fold --from '.^'
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing 77d787dfa5b6 "r5"
  merging mf
  rebasing dd0541003d21 "r6"
  merging mf
  rebasing fac8d040c80b "r2"
  merging mf
  rebasing c480ccdc36c0 "r4"
  merging mf
  $ showgraph
  o  6b8dd87db039 r4
  │
  o  7fd219543f4f r2
  │
  │ o  31892267fa07 r6
  │ │
  │ o  c74bb9c4eec9 r5
  ├─╯
  @  bfc9ee54b8f4 r0

Test folding multiple changesets, using default behavior of folding
up to working copy parent. Also tests situation where the branch to
rebase is not on the topmost folded commit.
  $ reset
  $ hg debugbuilddag -m "+5 *4 +2"
  $ showgraph
  o  9c9414e0356c r7
  │
  o  ec6d8e65acbe r6
  │
  o  77d787dfa5b6 r5
  │
  │ o  b762560d23fd r4
  │ │
  │ o  a422badec216 r3
  │ │
  │ o  37d4c1cec295 r2
  ├─╯
  o  f177fbb9e8d1 r1
  │
  o  93cbaf5e6529 r0

  $ hg up 'desc(r0)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg fold --from 'desc(r2)'
  3 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing a422badec216 "r3"
  merging mf
  rebasing b762560d23fd "r4"
  merging mf
  rebasing 77d787dfa5b6 "r5"
  merging mf
  rebasing ec6d8e65acbe "r6"
  merging mf
  rebasing 9c9414e0356c "r7"
  merging mf
  $ showgraph
  o  69b8281910bd r7
  │
  o  b494e86b0fcd r6
  │
  o  3b418b2dcaeb r5
  │
  │ o  a78a744630c5 r4
  │ │
  │ o  6032a3d5c310 r3
  ├─╯
  @  001b0872b432 r0

Test folding changesets unrelated to working copy parent using --exact.
Also test that using node hashes instead of rev numbers works.
  $ reset
  $ hg debugbuilddag -m +6
  $ showgraph
  o  f2987ebe5838 r5
  │
  o  aa70f0fe546a r4
  │
  o  cb14eba0ad9c r3
  │
  o  f07e66f449d0 r2
  │
  o  09bb8c08de89 r1
  │
  o  fdaccbb26270 r0

  $ hg fold --exact 09bb8c f07e66 cb14eb
  3 changesets folded
  rebasing aa70f0fe546a "r4"
  merging mf
  rebasing f2987ebe5838 "r5"
  merging mf
  $ showgraph
  o  064f4cd2992f r5
  │
  o  e39a86ad4ff1 r4
  │
  o  b36e18e69785 r1
  │
  o  fdaccbb26270 r0

Test --no-rebase flag.
  $ hg fold --no-rebase --exact 6 7
  2 changesets folded
  $ showgraph
  o  b431410f50a9 r1
  │
  │ o  064f4cd2992f r5
  │ │
  │ x  e39a86ad4ff1 r4
  │ │
  │ x  b36e18e69785 r1
  ├─╯
  o  fdaccbb26270 r0

Test that bookmarks are correctly moved.
  $ reset
  $ hg debugbuilddag +3
  $ hg bookmarks -r 'desc(r1)' test1
  $ hg bookmarks -r 'desc(r2)' test2_1
  $ hg bookmarks -r 'desc(r2)' test2_2
  $ hg bookmarks
     test1                     66f7d451a68b
     test2_1                   01241442b3c2
     test2_2                   01241442b3c2
  $ hg fold --exact 1 2
  2 changesets folded
  $ hg bookmarks
     test1                     ea7cac362d6c
     test2_1                   ea7cac362d6c
     test2_2                   ea7cac362d6c

Test JSON output
  $ reset
  $ hg debugbuilddag -m +6
  $ hg up 'desc(r5)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  f2987ebe5838 r5
  │
  o  aa70f0fe546a r4
  │
  o  cb14eba0ad9c r3
  │
  o  f07e66f449d0 r2
  │
  o  09bb8c08de89 r1
  │
  o  fdaccbb26270 r0

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
  @  befa2830d024 r3
  │
  o  f07e66f449d0 r2
  │
  o  09bb8c08de89 r1
  │
  o  fdaccbb26270 r0

XXX: maybe we also want the rebase nodechanges here.
When rebase is involved
  $ hg fold --exact 1 f07e66f449d06b214d0a8a9b1a6fa8af2f5f79a5 -Tjson -q
  [
   {
    "count": 2,
    "nodechanges": {"09bb8c08de89bca9fffcd6ed3530d6178f07d9e2": ["d65bf110c68ee2cf0a0ba076da90df3fcf76229b"], "f07e66f449d06b214d0a8a9b1a6fa8af2f5f79a5": ["d65bf110c68ee2cf0a0ba076da90df3fcf76229b"]}
   }
  ]

  $ hg fold --exact 0 d65bf110c68ee2cf0a0ba076da90df3fcf76229b -T '{nodechanges|json}' -q
  {"d65bf110c68ee2cf0a0ba076da90df3fcf76229b": ["785c10c9aad58fba814a235f074a79bdc5535083"], "fdaccbb26270c9a42503babe11fd846d7300df0b": ["785c10c9aad58fba814a235f074a79bdc5535083"]} (no-eol)

Test fold with --reuse-message
  $ reset
  $ hg debugbuilddag -m +6
  $ hg up 'desc(r5)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg fold --from 'desc(r1)' --reuse-message 'desc(r3)'
  5 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  e45dfaa6fe9c r3
  │
  o  fdaccbb26270 r0

