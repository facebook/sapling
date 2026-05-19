
#require no-eden


  $ setconfig devel.segmented-changelog-rev-compat=true

  $ HGMERGE=true; export HGMERGE

init

  $ sl init repo
  $ cd repo

commit

  $ echo 'a' > a
  $ sl ci -A -m test -u nobody -d '1 0'
  adding a

annotate -c

  $ sl annotate -c a
  8435f90966e4: a

annotate -cl

  $ sl annotate -cl a
  8435f90966e4:1: a

annotate -d

  $ sl annotate -d a
  Thu Jan 01 00:00:01 1970 +0000: a

annotate -n

  $ sl annotate -n a
  0: a

annotate -nl

  $ sl annotate -nl a
  0:1: a

annotate -u

  $ sl annotate -u a
  nobody: a

annotate -cdnu

  $ sl annotate -cdnu a
  nobody 0 8435f90966e4 Thu Jan 01 00:00:01 1970 +0000: a

annotate -cdnul

  $ sl annotate -cdnul a
  nobody 0 8435f90966e4 Thu Jan 01 00:00:01 1970 +0000:1: a

annotate (JSON)

  $ sl annotate -Tjson a
  [
   {
    "abspath": "a",
    "lines": [{"age_bucket": "old", "line": "a\n", "node": "8435f90966e442695d2ded29fdade2bac5ad8065"}],
    "path": "a"
   }
  ]

  $ sl annotate -Tjson -cdfnul a
  [
   {
    "abspath": "a",
    "lines": [{"age_bucket": "old", "date": [1.0, 0], "file": "a", "line": "a\n", "line_number": 1, "node": "8435f90966e442695d2ded29fdade2bac5ad8065", "rev": 0, "user": "nobody"}],
    "path": "a"
   }
  ]

  $ cat <<EOF >>a
  > a
  > a
  > EOF
  $ sl ci -ma1 -d '1 0'
  $ sl cp a b
  $ sl ci -mb -d '1 0'
  $ cat <<EOF >> b
  > b4
  > b5
  > b6
  > EOF
  $ sl ci -mb2 -d '2 0'

annotate multiple files (JSON)

  $ sl annotate -Tjson a b
  [
   {
    "abspath": "a",
    "lines": [{"age_bucket": "old", "line": "a\n", "node": "8435f90966e442695d2ded29fdade2bac5ad8065"}, {"age_bucket": "old", "line": "a\n", "node": "762f04898e6684ff713415f7b8a8d53d33f96c92"}, {"age_bucket": "old", "line": "a\n", "node": "762f04898e6684ff713415f7b8a8d53d33f96c92"}],
    "path": "a"
   },
   {
    "abspath": "b",
    "lines": [{"age_bucket": "old", "line": "a\n", "node": "8435f90966e442695d2ded29fdade2bac5ad8065"}, {"age_bucket": "old", "line": "a\n", "node": "762f04898e6684ff713415f7b8a8d53d33f96c92"}, {"age_bucket": "old", "line": "a\n", "node": "762f04898e6684ff713415f7b8a8d53d33f96c92"}, {"age_bucket": "old", "line": "b4\n", "node": "37ec9f5c3d1f99572d7075971cb4876e2139b52f"}, {"age_bucket": "old", "line": "b5\n", "node": "37ec9f5c3d1f99572d7075971cb4876e2139b52f"}, {"age_bucket": "old", "line": "b6\n", "node": "37ec9f5c3d1f99572d7075971cb4876e2139b52f"}],
    "path": "b"
   }
  ]

annotate multiple files (template)

  $ sl annotate -T'== {abspath} ==\n{lines % "{line}"}' a b
  == a ==
  a
  a
  a
  == b ==
  a
  a
  a
  b4
  b5
  b6

annotate -n b

  $ sl annotate -n b
  0: a
  1: a
  1: a
  3: b4
  3: b5
  3: b6

annotate --no-follow b

  $ sl annotate --no-follow b
  3086dbafde1c: a
  3086dbafde1c: a
  3086dbafde1c: a
  37ec9f5c3d1f: b4
  37ec9f5c3d1f: b5
  37ec9f5c3d1f: b6

annotate -nl b

  $ sl annotate -nl b
  0:1: a
  1:2: a
  1:3: a
  3:4: b4
  3:5: b5
  3:6: b6

annotate -nf b

  $ sl annotate -nf b
  0 a: a
  1 a: a
  1 a: a
  3 b: b4
  3 b: b5
  3 b: b6

annotate -nlf b

  $ sl annotate -nlf b
  0 a:1: a
  1 a:2: a
  1 a:3: a
  3 b:4: b4
  3 b:5: b5
  3 b:6: b6

  $ sl up -C 3086dbafde1ce745abfc8d2d367847280aabae9d
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat <<EOF >> b
  > b4
  > c
  > b5
  > EOF
  $ sl ci -mb2.1 -d '2 0'
  $ sl merge
  merging b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl ci -mmergeb -d '3 0'

annotate after merge

  $ sl annotate -nf b
  0 a: a
  1 a: a
  1 a: a
  3 b: b4
  4 b: c
  3 b: b5

annotate after merge with -l

  $ sl annotate -nlf b
  0 a:1: a
  1 a:2: a
  1 a:3: a
  3 b:4: b4
  4 b:5: c
  3 b:5: b5

  $ sl up -C 'desc(a1)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl cp a b
  $ cat <<EOF > b
  > a
  > z
  > a
  > EOF
  $ sl ci -mc -d '3 0'
  $ sl merge
  merging b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat <<EOF >> b
  > b4
  > c
  > b5
  > EOF
  $ echo d >> b
  $ sl ci -mmerge2 -d '4 0'

annotate after rename merge

  $ sl annotate -nf b
  0 a: a
  6 b: z
  1 a: a
  3 b: b4
  4 b: c
  3 b: b5
  7 b: d

annotate after rename merge with -l

  $ sl annotate -nlf b
  0 a:1: a
  6 b:2: z
  1 a:3: a
  3 b:4: b4
  4 b:5: c
  3 b:5: b5
  7 b:7: d

Issue2807: alignment of line numbers with -l

  $ echo more >> b
  $ sl ci -mmore -d '5 0'
  $ echo more >> b
  $ sl ci -mmore -d '6 0'
  $ echo more >> b
  $ sl ci -mmore -d '7 0'
  $ sl annotate -nlf b
   0 a: 1: a
   6 b: 2: z
   1 a: 3: a
   3 b: 4: b4
   4 b: 5: c
   3 b: 5: b5
   7 b: 7: d
   8 b: 8: more
   9 b: 9: more
  10 b:10: more

linkrev vs rev

  $ sl annotate -r tip -n a
  0: a
  1: a
  1: a

linkrev vs rev with -l

  $ sl annotate -r tip -nl a
  0:1: a
  1:2: a
  1:3: a

Issue589: "undelete" sequence leads to crash

annotate was crashing when trying to --follow something

like A -> B -> A

generate ABA rename configuration

  $ echo foo > foo
  $ sl add foo
  $ sl ci -m addfoo
  $ sl rename foo bar
  $ sl ci -m renamefoo
  $ sl rename bar foo
  $ sl ci -m renamebar

annotate after ABA with follow

  $ sl annotate --file foo
  foo: foo

missing file

  $ sl ann nosuchfile
  abort: nosuchfile: no such file in rev e9e6b4fa872f
  [255]

annotate file without '\n' on last line

  $ printf "" > c
  $ sl ci -A -m test -u nobody -d '1 0'
  adding c
  $ sl annotate c
  $ printf "a\nb" > c
  $ sl ci -m test
  $ sl annotate c
  8c47368c200b: a
  8c47368c200b: b

Issue3841: check annotation of the file of which filelog includes
merging between the revision and its ancestor

to reproduce the situation with recent Mercurial, this script uses (1)
"sl debugsetparents" to merge without ancestor check by "sl merge",
and (2) the extension to allow filelog merging between the revision
and its ancestor by overriding "repo._filecommit".

  $ cat > ../legacyrepo.py <<EOF
  > from __future__ import absolute_import
  > from sapling import error, node
  > def reposetup(ui, repo):
  >     class legacyrepo(repo.__class__):
  >         def _filecommit(self, fctx, manifest1, manifest2,
  >                         linkrev, tr, changelist):
  >             fname = fctx.path()
  >             text = fctx.data()
  >             flog = self.file(fname)
  >             fparent1 = manifest1.get(fname, node.nullid)
  >             fparent2 = manifest2.get(fname, node.nullid)
  >             meta = {}
  >             copy = fctx.renamed()
  >             if copy and copy[0] != fname:
  >                 raise error.Abort('copying is not supported')
  >             if fparent2 != node.nullid:
  >                 changelist.append(fname)
  >                 return flog.add(text, meta, tr, linkrev,
  >                                 fparent1, fparent2)
  >             raise error.Abort('only merging is supported')
  >     repo.__class__ = legacyrepo
  > EOF

  $ cat > baz <<EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ sl add baz
  $ sl commit -m "baz:0"

  $ cat > baz <<EOF
  > 1 baz:1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ sl commit -m "baz:1"

  $ cat > baz <<EOF
  > 1 baz:1
  > 2 baz:2
  > 3
  > 4
  > 5
  > EOF
  $ sl debugsetparents 933981f264573acb5782b58f8f6fba0f5c815ac7 933981f264573acb5782b58f8f6fba0f5c815ac7
  $ sl --config extensions.legacyrepo=../legacyrepo.py  commit -m "baz:2"
  $ sl annotate baz
  933981f26457: 1 baz:1
  be4ba992a055: 2 baz:2
  6bf217a7698a: 3
  6bf217a7698a: 4
  6bf217a7698a: 5

  $ cat > baz <<EOF
  > 1 baz:1
  > 2 baz:2
  > 3 baz:3
  > 4
  > 5
  > EOF
  $ sl commit -m "baz:3"

  $ cat > baz <<EOF
  > 1 baz:1
  > 2 baz:2
  > 3 baz:3
  > 4 baz:4
  > 5
  > EOF
  $ sl debugsetparents 79574f0f4414c85637f114949d21baf1e189f7fa be4ba992a05544692d87c941b05d044a3ebe48a0
  $ sl --config extensions.legacyrepo=../legacyrepo.py  commit -m "baz:4"
  $ sl annotate baz
  933981f26457: 1 baz:1
  be4ba992a055: 2 baz:2
  79574f0f4414: 3 baz:3
  3a681db4976d: 4 baz:4
  6bf217a7698a: 5

annotate clean file

  $ sl annotate -ncr "wdir()" foo
  11 472b18db256d: foo

annotate modified file

  $ echo foofoo >> foo
  $ sl annotate -r "wdir()" foo
  472b18db256d : foo
  3a681db4976d+: foofoo

  $ sl annotate -cr "wdir()" foo
  472b18db256d : foo
  3a681db4976d+: foofoo

  $ sl annotate -ncr "wdir()" foo
  11 472b18db256d : foo
  20 3a681db4976d+: foofoo

  $ sl annotate --debug -ncr "wdir()" foo
  11 472b18db256d1e8282064eab4bfdaf48cbfe83cd : foo
  20 3a681db4976d5f6c78ca87a4d6f933ff7867ccca+: foofoo

  $ sl annotate -udr "wdir()" foo
  test Thu Jan 01 00:00:00 1970 +0000: foo
  test [A-Za-z0-9:+ ]+: foofoo (re)

  $ sl annotate -ncr "wdir()" -Tjson foo
  [
   {
    "abspath": "foo",
    "lines": [{"age_bucket": "old", "line": "foo\n", "node": "472b18db256d1e8282064eab4bfdaf48cbfe83cd", "rev": 11}, {"age_bucket": "1hour", "line": "foofoo\n", "node": null, "rev": null}],
    "path": "foo"
   }
  ]

annotate added file

  $ echo bar > bar
  $ sl add bar
  $ sl annotate -ncr "wdir()" bar
  20 3a681db4976d+: bar

annotate renamed file

  $ sl rename foo renamefoo2
  $ sl annotate -ncr "wdir()" renamefoo2
  11 472b18db256d : foo
  20 3a681db4976d+: foofoo

annotate missing file

  $ rm baz

  $ sl annotate -ncr "wdir()" baz
  abort: $ENOENT$: baz
  [255]

annotate removed file

  $ sl rm baz

  $ sl annotate -ncr "wdir()" baz
  abort: $ENOENT$: baz
  [255]

  $ sl revert --all --no-backup --quiet
  $ sl id -n
  20

Test empty annotate output

  $ printf '\0' > binary
  $ touch empty
  $ sl ci -qAm 'add binary and empty files'

  $ sl annotate binary empty
  binary: binary file

  $ sl annotate -Tjson binary empty
  [
   {
    "abspath": "binary",
    "path": "binary"
   },
   {
    "abspath": "empty",
    "lines": [],
    "path": "empty"
   }
  ]

Test annotate with whitespace options

  $ cd ..
  $ sl init repo-ws
  $ cd repo-ws
  $ cat > a <<EOF
  > aa
  > 
  > b b
  > EOF
  $ sl ci -Am "adda"
  adding a
  $ sed 's/EOL$//g' > a <<EOF
  > a  a
  > 
  >  EOL
  > b  b
  > EOF
  $ sl ci -m "changea"

Annotate with no option

  $ sl annotate a
  08f1b20a6199: a  a
  9ba9c410f1ce: 
  08f1b20a6199:  
  08f1b20a6199: b  b

Annotate with --ignore-space-change

  $ sl annotate --ignore-space-change a
  08f1b20a6199: a  a
  08f1b20a6199: 
  9ba9c410f1ce:  
  9ba9c410f1ce: b  b

Annotate with --ignore-all-space

  $ sl annotate --ignore-all-space a
  9ba9c410f1ce: a  a
  9ba9c410f1ce: 
  08f1b20a6199:  
  9ba9c410f1ce: b  b

Annotate with --ignore-blank-lines (similar to no options case)

  $ sl annotate --ignore-blank-lines a
  08f1b20a6199: a  a
  9ba9c410f1ce: 
  08f1b20a6199:  
  08f1b20a6199: b  b

  $ cd ..

Annotate with linkrev pointing to another branch
------------------------------------------------

create history with a filerev whose linkrev points to another branch

  $ sl init branchedlinkrev
  $ cd branchedlinkrev
  $ echo A > a
  $ sl commit -Am 'contentA'
  adding a
  $ echo B >> a
  $ sl commit -m 'contentB'
  $ sl up --rev 'desc(contentA)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo unrelated > unrelated
  $ sl commit -Am 'unrelated'
  adding unrelated
  $ sl graft -r 'desc(contentB)'
  grafting fd27c222e3e6 "contentB"
  $ echo C >> a
  $ sl commit -m 'contentC'
  $ echo W >> a
  $ sl log -G
  @  commit:      072f1e8df249
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     contentC
  │
  o  commit:      ff38df03cc4b
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     contentB
  │
  o  commit:      62aaf3f6fc06
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     unrelated
  │
  │ o  commit:      fd27c222e3e6
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     contentB
  │
  o  commit:      f0932f74827e
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     contentA
  

Annotate should list ancestor of starting revision only

  $ sl annotate a
  f0932f74827e: A
  ff38df03cc4b: B
  072f1e8df249: C

  $ sl annotate a -r 'wdir()'
  f0932f74827e : A
  ff38df03cc4b : B
  072f1e8df249 : C
  072f1e8df249+: W

Even when the starting revision is the linkrev-shadowed one:

  $ sl annotate a -r 'max(desc(contentB))'
  f0932f74827e: A
  ff38df03cc4b: B

  $ cd ..

Issue5360: Deleted chunk in p1 of a merge changeset

  $ sl init repo-5360
  $ cd repo-5360
  $ echo 1 > a
  $ sl commit -A a -m 1
  $ echo 2 >> a
  $ sl commit -m 2
  $ echo a > a
  $ sl commit -m a
  $ sl goto '.^' -q
  $ echo 3 >> a
  $ sl commit -m 3 -q
  $ sl merge 'desc(a)' -q
  $ cat > a << EOF
  > b
  > 1
  > 2
  > 3
  > a
  > EOF
  $ sl resolve --mark -q
  $ sl commit -m m
  $ sl annotate a
  af87e62e663e: b
  eff892de26ec: 1
  1ed24be7e7a0: 2
  0a068f0261cf: 3
  9409851bc20a: a

  $ cd ..
