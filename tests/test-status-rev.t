Tests of 'hg status --rev <rev>' to make sure status between <rev> and '.' get
combined correctly with the dirstate status.

Sets up a history for a number of files where the filename describes the file's
history. The first two letters of the filename describe the first two commits;
the third letter describes the dirstate for the file. For example, a file called
'amr' was added in the first commit, modified in the second and then removed in
the dirstate.

These codes are used for commits:
x: does not exist
a: added
c: clean
m: modified
r: removed

These codes are used for dirstate:
d: in dirstate, but deleted from disk
f: removed from dirstate, but file exists (forgotten)
r: removed from dirstate and disk
q: added, but deleted from disk (q for q-rious?)
u: not in dirstate, but file exists (unknown)

  $ hg init
  $ touch .hgignore
  $ hg add .hgignore
  $ hg commit -m initial

First letter: first commit

  $ echo a >acc
  $ echo a >acd
  $ echo a >acf
  $ echo a >acm
  $ echo a >acr
  $ echo a >amc
  $ echo a >amd
  $ echo a >amf
  $ echo a >amm
  $ echo a >amr
  $ echo a >ara
  $ echo a >arq
  $ echo a >aru
  $ hg commit -Aqm first

Second letter: second commit

  $ echo b >xad
  $ echo b >xaf
  $ echo b >xam
  $ echo b >xar
  $ echo b >amc
  $ echo b >amd
  $ echo b >amf
  $ echo b >amm
  $ echo b >amr
  $ hg rm ara
  $ hg rm arq
  $ hg rm aru
  $ hg commit -Aqm second

Third letter: dirstate

  $ echo c >acm
  $ echo c >amm
  $ echo c >xam
  $ echo c >ara && hg add ara
  $ echo c >arq && hg add arq && rm arq
  $ echo c >aru
  $ hg rm amr
  $ hg rm acr
  $ hg rm xar
  $ rm acd
  $ rm amd
  $ rm xad
  $ hg forget acf
  $ hg forget amf
  $ hg forget xaf
  $ touch xxu

Status compared to one revision back

  $ hg status -A --rev 1 acc
  C acc
BROKEN: file appears twice; should be '!'
  $ hg status -A --rev 1 acd
  ! acd
  C acd
  $ hg status -A --rev 1 acf
  R acf
  $ hg status -A --rev 1 acm
  M acm
  $ hg status -A --rev 1 acr
  R acr
  $ hg status -A --rev 1 amc
  M amc
BROKEN: file appears twice; should be '!'
  $ hg status -A --rev 1 amd
  ! amd
  C amd
  $ hg status -A --rev 1 amf
  R amf
  $ hg status -A --rev 1 amm
  M amm
  $ hg status -A --rev 1 amr
  R amr
  $ hg status -A --rev 1 ara
  M ara
BROKEN: file appears twice; should be '!'
  $ hg status -A --rev 1 arq
  R arq
  ! arq
  $ hg status -A --rev 1 aru
  R aru
  $ hg status -A --rev 1 xad
  ! xad
  $ hg status -A --rev 1 xaf
  $ hg status -A --rev 1 xam
  A xam
  $ hg status -A --rev 1 xar
  $ hg status -A --rev 1 xxu
  ? xxu

Status compared to two revisions back

  $ hg status -A --rev 0 acc
  A acc
  $ hg status -A --rev 0 acd
  ! acd
BROKEN: file exists, so should be listed (as '?')
  $ hg status -A --rev 0 acf
  $ hg status -A --rev 0 acm
  A acm
  $ hg status -A --rev 0 acr
  $ hg status -A --rev 0 amc
  A amc
  $ hg status -A --rev 0 amd
  ! amd
BROKEN: file exists, so should be listed (as '?')
  $ hg status -A --rev 0 amf
  $ hg status -A --rev 0 amm
  A amm
  $ hg status -A --rev 0 amr
  $ hg status -A --rev 0 ara
  A ara
  $ hg status -A --rev 0 arq
  ! arq
  $ hg status -A --rev 0 aru
  ? aru
  $ hg status -A --rev 0 xad
  ! xad
BROKEN: file exists, so should be listed (as '?')
  $ hg status -A --rev 0 xaf
  $ hg status -A --rev 0 xam
  A xam
  $ hg status -A --rev 0 xar
