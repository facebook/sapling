#chg-compatible

  $ setconfig extensions.treemanifest=!
prepare repo

  $ hg init a
  $ cd a
  $ echo "some text" > FOO.txt
  $ echo "another text" > bar.txt
  $ echo "more text" > QUICK.txt
  $ hg add
  adding FOO.txt
  adding QUICK.txt
  adding bar.txt
  $ hg ci -mtest1

verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 1 changesets, 3 total revisions

verify current commit

  $ hg verify -r '.'
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 1 changesets, 3 total revisions

verify all commits using rev range

  $ hg verify -r ':'
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 1 changesets, 3 total revisions

verify with journal

  $ touch .hg/store/journal
  $ hg verify
  abandoned transaction found - run hg recover
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 1 changesets, 3 total revisions
  $ rm .hg/store/journal

introduce some bugs in repo

  $ cd .hg/store/data
  $ mv _f_o_o.txt.i X_f_o_o.txt.i
  $ mv bar.txt.i xbar.txt.i
  $ rm _q_u_i_c_k.txt.i

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   warning: revlog 'data/FOO.txt.i' not in fncache!
   0: empty or missing FOO.txt
   FOO.txt@0: manifest refers to unknown revision f62022d3d590
   warning: revlog 'data/QUICK.txt.i' not in fncache!
   0: empty or missing QUICK.txt
   QUICK.txt@0: manifest refers to unknown revision 88b857db8eba
   warning: revlog 'data/bar.txt.i' not in fncache!
   0: empty or missing bar.txt
   bar.txt@0: manifest refers to unknown revision 256559129457
  3 files, 1 changesets, 0 total revisions
  3 warnings encountered!
  hint: run "hg debugrebuildfncache" to recover from corrupt fncache
  6 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]

  $ hg verify -r '.'
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   0: empty or missing FOO.txt
   FOO.txt@0: manifest refers to unknown revision f62022d3d590
   0: empty or missing QUICK.txt
   QUICK.txt@0: manifest refers to unknown revision 88b857db8eba
   0: empty or missing bar.txt
   bar.txt@0: manifest refers to unknown revision 256559129457
  3 files, 1 changesets, 0 total revisions
  6 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]

  $ cd ../../..
  $ cd ..

Set up a repo for testing missing revlog entries

  $ hg init missing-entries
  $ cd missing-entries
  $ echo 0 > file
  $ hg ci -Aqm0
  $ cp -R .hg/store .hg/store-partial
  $ echo 1 > file
  $ hg ci -Aqm1
  $ cp -R .hg/store .hg/store-full

Entire changelog missing

  $ rm .hg/store/00changelog.*
  $ hg verify -q
   0: empty or missing changelog
   manifest@0: d0b6632564d4 not in changesets
   manifest@1: 941fc4534185 not in changesets
  3 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cp -R .hg/store-full/. .hg/store

Entire manifest log missing

  $ rm .hg/store/00manifest.*
  $ hg verify -q
   0: empty or missing manifest
  1 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ hg verify -q -r '.'
   0: empty or missing manifest
  1 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ hg verify -q -r '.^'
   0: empty or missing manifest
  1 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cp -R .hg/store-full/. .hg/store

Entire filelog missing

  $ rm .hg/store/data/file.*
  $ hg verify -q
   warning: revlog 'data/file.i' not in fncache!
   0: empty or missing file
   file@0: manifest refers to unknown revision 362fef284ce2
   file@1: manifest refers to unknown revision c10f2164107d
  1 warnings encountered!
  hint: run "hg debugrebuildfncache" to recover from corrupt fncache
  3 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ hg verify -q -r '.'
   1: empty or missing file
   file@1: manifest refers to unknown revision c10f2164107d
  2 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]
  $ hg verify -q -r ':tip'
   0: empty or missing file
   file@0: manifest refers to unknown revision 362fef284ce2
   file@1: manifest refers to unknown revision c10f2164107d
  3 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cp -R .hg/store-full/. .hg/store

Entire changelog and manifest log missing

  $ rm .hg/store/00changelog.*
  $ rm .hg/store/00manifest.*
  $ hg verify -q
  warning: orphan revlog 'data/file.i'
  1 warnings encountered!
  $ cp -R .hg/store-full/. .hg/store

Entire changelog and filelog missing

  $ rm .hg/store/00changelog.*
  $ rm .hg/store/data/file.*
  $ hg verify -q
   0: empty or missing changelog
   manifest@0: d0b6632564d4 not in changesets
   manifest@1: 941fc4534185 not in changesets
   warning: revlog 'data/file.i' not in fncache!
   ?: empty or missing file
   file@0: manifest refers to unknown revision 362fef284ce2
   file@1: manifest refers to unknown revision c10f2164107d
  1 warnings encountered!
  hint: run "hg debugrebuildfncache" to recover from corrupt fncache
  6 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cp -R .hg/store-full/. .hg/store

Entire manifest log and filelog missing

  $ rm .hg/store/00manifest.*
  $ rm .hg/store/data/file.*
  $ hg verify -q
   0: empty or missing manifest
   warning: revlog 'data/file.i' not in fncache!
   0: empty or missing file
  1 warnings encountered!
  hint: run "hg debugrebuildfncache" to recover from corrupt fncache
  2 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ hg verify -q -r '.'
   0: empty or missing manifest
   1: empty or missing file
  2 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cp -R .hg/store-full/. .hg/store

Changelog missing entry

  $ cp -f .hg/store-partial/00changelog.* .hg/store
  $ hg verify -q
   manifest@?: rev 1 points to nonexistent changeset 1
   manifest@?: 941fc4534185 not in changesets
   file@?: rev 1 points to nonexistent changeset 1
   (expected 0)
  1 warnings encountered!
  3 integrity errors encountered!
  [1]
  $ hg verify -q -r 0
  warning: failed to inspect working copy parent
  warning: failed to inspect working copy parent
  $ hg verify -q -r 1
  warning: failed to inspect working copy parent
  abort: 00changelog.i@c5ddb05ab828: no node!
  [255]
  $ cp -R .hg/store-full/. .hg/store

Manifest log missing entry

  $ cp -f .hg/store-partial/00manifest.* .hg/store
  $ hg verify -q
   manifest@1: changeset refers to unknown revision 941fc4534185
   file@1: c10f2164107d not in manifests
  2 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]
  $ hg verify -q -r '.'
   manifest@1: changeset refers to unknown revision 941fc4534185
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]
  $ hg verify -q -r '.^'
  $ cp -R .hg/store-full/. .hg/store

Filelog missing entry

  $ cp -f .hg/store-partial/data/file.* .hg/store/data
  $ hg verify -q
   file@1: manifest refers to unknown revision c10f2164107d
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]
  $ hg verify -q -r'.'
   file@1: manifest refers to unknown revision c10f2164107d
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]
  $ hg verify -q -r'.^'
  $ cp -R .hg/store-full/. .hg/store

Changelog and manifest log missing entry

  $ cp -f .hg/store-partial/00changelog.* .hg/store
  $ cp -f .hg/store-partial/00manifest.* .hg/store
  $ hg verify -q
   file@?: rev 1 points to nonexistent changeset 1
   (expected 0)
   file@?: c10f2164107d not in manifests
  1 warnings encountered!
  2 integrity errors encountered!
  [1]
  $ hg verify -q -r 0
  warning: failed to inspect working copy parent
  warning: failed to inspect working copy parent
  $ hg verify -q -r 1
  warning: failed to inspect working copy parent
  abort: 00changelog.i@c5ddb05ab828: no node!
  [255]
  $ cp -R .hg/store-full/. .hg/store

Changelog and filelog missing entry

  $ cp -f .hg/store-partial/00changelog.* .hg/store
  $ cp -f .hg/store-partial/data/file.* .hg/store/data
  $ hg verify -q
   manifest@?: rev 1 points to nonexistent changeset 1
   manifest@?: 941fc4534185 not in changesets
   file@?: manifest refers to unknown revision c10f2164107d
  3 integrity errors encountered!
  [1]
  $ hg verify -q -r 0
  warning: failed to inspect working copy parent
  warning: failed to inspect working copy parent
  $ hg verify -q -r 1
  warning: failed to inspect working copy parent
  abort: 00changelog.i@c5ddb05ab828: no node!
  [255]
  $ cp -R .hg/store-full/. .hg/store

Manifest and filelog missing entry

  $ cp -f .hg/store-partial/00manifest.* .hg/store
  $ cp -f .hg/store-partial/data/file.* .hg/store/data
  $ hg verify -q
   manifest@1: changeset refers to unknown revision 941fc4534185
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]
  $ hg verify -q -r '.'
   manifest@1: changeset refers to unknown revision 941fc4534185
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]
  $ hg verify -q -r '.^'
  $ cp -R .hg/store-full/. .hg/store

Corrupt changelog base node to cause failure to read revision

  $ printf abcd | dd conv=notrunc of=.hg/store/00changelog.i bs=1 seek=16 \
  >   2> /dev/null
  $ hg verify -q
   0: unpacking changeset 08b1860757c2: * (glob)
   manifest@?: rev 0 points to unexpected changeset 0
   manifest@?: d0b6632564d4 not in changesets
   file@?: rev 0 points to unexpected changeset 0
   (expected 1)
  1 warnings encountered!
  4 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ hg verify -q -r '.'
  $ hg verify -q -r '.^'
   0: unpacking changeset 08b1860757c2: * (glob)
   manifest@?: rev 0 points to unexpected changeset 0
   manifest@?: d0b6632564d4 not in changesets
   file@0: in manifest but not in changeset
   file@?: rev 0 points to unexpected changeset 0
  5 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cp -R .hg/store-full/. .hg/store

Corrupt manifest log base node to cause failure to read revision

  $ printf abcd | dd conv=notrunc of=.hg/store/00manifest.i bs=1 seek=16 \
  >   2> /dev/null
  $ hg verify -q
   manifest@0: reading delta d0b6632564d4: * (glob)
   manifest@1: reading delta 941fc4534185: revision outside index
   file@0: in changeset but not in manifest
  3 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ hg verify -q -r '.'
   manifest@1: reading delta 941fc4534185: revision outside index
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]
  $ hg verify -q -r '.^'
   manifest@0: reading delta d0b6632564d4: * (glob)
  1 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cp -R .hg/store-full/. .hg/store

Corrupt filelog base node to cause failure to read revision

  $ printf abcd | dd conv=notrunc of=.hg/store/data/file.i bs=1 seek=16 \
  >   2> /dev/null
  $ hg verify -q
   file@0: unpacking 362fef284ce2: * (glob)
  1 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ hg verify -q -r '.'
  $ hg verify -q -r '.^'
   file@0: unpacking 362fef284ce2: * (glob)
  1 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cp -R .hg/store-full/. .hg/store

  $ cd ..

test changelog without a manifest

  $ hg init b
  $ cd b
  $ hg ci -m foo --config ui.allowemptycommit=1
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  0 files, 1 changesets, 0 total revisions

test revlog corruption

  $ touch a
  $ hg add a
  $ hg ci -m a

  $ echo 'corrupted' > b
  $ dd if=.hg/store/data/a.i of=start bs=1 count=20 2>/dev/null
  $ cat start b > .hg/store/data/a.i

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   a@1: broken revlog! (index data/a.i is corrupted)
  warning: orphan revlog 'data/a.i'
  1 files, 2 changesets, 0 total revisions
  1 warnings encountered!
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]

  $ hg verify -r '.'
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   a@1: broken revlog! (index data/a.i is corrupted)
  1 files, 1 changesets, 0 total revisions
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]

  $ hg verify -r '.^'
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  0 files, 1 changesets, 0 total revisions

  $ cd ..

test revlog format 0

  $ revlog-formatv0.py
  $ cd formatv0
  $ hg verify
  abort: legacy dirstate implementations are no longer supported!
  [255]
  $ cd ..

test flag processor and skipflags

  $ hg init skipflags
  $ cd skipflags
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > flagprocessor=$RUNTESTDIR/flagprocessorext.py
  > EOF
  $ echo '[BASE64]content' > base64
  $ hg commit -Aqm 'flag processor content' base64
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

  $ cat >> $TESTTMP/break-base64.py <<EOF
  > from __future__ import absolute_import
  > import base64
  > base64.b64decode=lambda x: x
  > EOF
  $ cat >> .hg/hgrc <<EOF
  > breakbase64=$TESTTMP/break-base64.py
  > EOF

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   base64@0: unpacking 794cee7777cb: integrity check failed on data/base64.i:0
  1 files, 1 changesets, 1 total revisions
  1 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ hg verify --config verify.skipflags=2147483647
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

test verify a deleted file with rev range
  $ hg init deletefiles
  $ cd deletefiles
  $ echo "some text" > newfile.txt
  $ hg add
  adding newfile.txt
  $ hg ci -mtest1

  $ hg rm newfile.txt
  $ hg ci -mtest2

  $ hg verify -r tip
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 0 total revisions
