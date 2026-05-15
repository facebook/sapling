
#require no-eden

#inprocess-hg-incompatible


BUG: this shouldn't be necessary, but currently "sl add -s ..." or "sl sparse
include ..." doesn't work for untracked files not previously in the sparse
profile.
  $ setconfig fsmonitor.track-ignore-files=true

test sparse

  $ enable sparse
  $ newclientrepo myrepo
  $ enable sparse

  $ echo a > show
  $ echo x > hide
  $ sl ci -Aqm 'initial'

  $ echo b > show
  $ echo y > hide
  $ echo aa > show2
  $ echo xx > hide2
  $ sl ci -Aqm 'two'

Verify basic include

  $ sl up -q 'desc(initial)'
  $ sl sparse include 'hide'
  $ ls
  hide

Absolute paths outside the repo should just be rejected

  $ sl sparse include /foo/bar
  abort: paths cannot be absolute
  [255]
  $ sl sparse include '$TESTTMP/myrepo/hide'

  $ sl sparse include '/root'
  abort: paths cannot be absolute
  [255]

Repo root-relaive vs. cwd-relative includes
  $ mkdir subdir
  $ cd subdir
  $ sl sparse include --config sparse.includereporootpaths=on notinsubdir/path
  $ sl sparse include --config sparse.includereporootpaths=off **/path
  $ sl sparse include --config sparse.includereporootpaths=off path:abspath
  $ sl sparse
  [include]
  $TESTTMP/myrepo/hide
  hide
  notinsubdir/path
  path:abspath
  subdir/**/path
  [exclude]
  
  
  $ cd ..
  $ rm -rf subdir

Verify deleting uses relative paths
  $ mkdir subdir && echo foo > subdir/foo
  $ sl sparse
  [include]
  $TESTTMP/myrepo/hide
  hide
  notinsubdir/path
  path:abspath
  subdir/**/path
  [exclude]
  
  
  $ cd subdir
  $ sl sparse --delete **/path
  $ sl sparse
  [include]
  $TESTTMP/myrepo/hide
  hide
  notinsubdir/path
  path:abspath
  [exclude]
  
  
  $ cd ..
  $ rm -rf subdir

Verify commiting while sparse includes other files

  $ echo z > hide
  $ sl ci -Aqm 'edit hide'
  $ ls
  hide
  $ sl manifest
  hide
  show
  $ sl files
  hide

Verify --reset brings files back

  $ sl sparse --reset
  $ ls
  hide
  show
  $ cat hide
  z
  $ cat show
  a

Verify 'sl sparse' default output

  $ sl up -q null
  $ sl sparse include 'show*'

  $ sl sparse
  [include]
  show*
  [exclude]
  
  
Verify update only writes included files

  $ sl up -q 'desc(initial)'
  $ ls
  show

  $ sl up -q 'desc(two)'
  $ ls
  show
  show2

Verify status only shows included files

  $ touch hide
  $ touch hide3
  $ echo c > show
  $ sl status
  M show

Adding an excluded file should fail

  $ sl add hide3
  the following files are ignored, but still added because they are explicitly specified:
    hide3
  (use 'sl debugignore <file>' to check why they are ignored)
  abort: cannot add 'hide3' - it is outside the sparse checkout
  (include file with `sl sparse include <pattern>` or use `sl add -s <file>` to include file directory while adding)
  [255]

Verify deleting sparseness while a file has changes fails

  $ sl sparse uninclude 'show*'
  pending changes to 'hide'
  abort: cannot change sparseness due to pending changes (delete the files or use --force to bring them back dirty)
  [255]

Verify deleting sparseness with --force brings back files

  $ sl sparse uninclude -f 'show*'
  pending changes to 'hide'
  $ ls
  hide
  hide2
  hide3
  show
  show2
  $ sl st
  M hide
  M show
  ? hide3

Verify editing sparseness fails if pending changes

  $ sl sparse include 'show*'
  pending changes to 'hide'
  abort: could not update sparseness due to pending changes
  [255]

Verify adding sparseness hides files

  $ sl sparse exclude -f 'hide*'
  pending changes to 'hide'
  $ ls
  hide
  hide3
  show
  show2
  $ sl st
  M show

  $ sl up -qC .
  $ rm hide*
  $ ls
  show
  show2

Verify rebase temporarily includes excluded files

  $ sl rebase -d 'desc(two)' -r 'desc(edit)' --config extensions.rebase=
  rebasing b91df4f39e75 "edit hide"
  temporarily included 1 file(s) in the sparse checkout for merging
  merging hide
  warning: 1 conflicts while merging hide! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]

  $ sl sparse
  [include]
  
  [exclude]
  hide*
  
  Temporarily Included Files (for merge/rebase):
  hide

  $ cat hide
  <<<<<<< dest:   39278f7c08a9 - test: two
  y
  =======
  z
  >>>>>>> source: b91df4f39e75 - test: edit hide

Verify aborting a rebase cleans up temporary files

  $ sl rebase --abort --config extensions.rebase=
  cleaned up 1 temporarily added file(s) from the sparse checkout
  rebase aborted
  $ rm hide.orig

  $ ls
  show
  show2

Verify merge fails if merging excluded files

  $ sl up -q 'desc(two)'
  $ sl merge -r 'desc(edit)'
  temporarily included 1 file(s) in the sparse checkout for merging
  merging hide
  warning: 1 conflicts while merging hide! (edit, then use 'sl resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges or 'sl goto -C .' to abandon
  [1]
  $ sl sparse
  [include]
  
  [exclude]
  hide*
  
  Temporarily Included Files (for merge/rebase):
  hide

  $ sl up -C .
  cleaned up 1 temporarily added file(s) from the sparse checkout
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl sparse
  [include]
  
  [exclude]
  hide*
  

Verify strip -k resets dirstate correctly

  $ sl status
  $ sl sparse
  [include]
  
  [exclude]
  hide*
  
  $ sl log -r . -T '{node}\n' --stat
  39278f7c08a90f4978d008dd6edffc97ec308350
   hide  |  2 +-
   hide2 |  1 +
   show  |  2 +-
   show2 |  1 +
   4 files changed, 4 insertions(+), 2 deletions(-)
  
  $ sl debugstrip -r . -k
  $ sl status
  M show
  ? show2

Verify rebase succeeds if all changed files are in sparse checkout

  $ sl commit -Aqm "add show2"
  $ sl rebase -d 'desc(edit)' --config extensions.rebase=
  rebasing bdde55290160 "add show2"

Verify log --sparse only shows commits that affect the sparse checkout

  $ sl log -T '{node} '
  042e19a4662d597f8f14a73aaaf368bc9c992960 b91df4f39e75c7b8fd8b1c7c7fe5ccdbd1d9a76c 6ef42ed31c865363d437a0a3455fed45f80b5dfd  (no-eol)
  $ sl log --sparse -T '{node} '
  042e19a4662d597f8f14a73aaaf368bc9c992960 6ef42ed31c865363d437a0a3455fed45f80b5dfd  (no-eol)

Test status on a file in a subdir

  $ mkdir -p dir1/dir2
  $ touch dir1/dir2/file
  $ sl sparse -I dir1/dir2
  $ sl status
  ? dir1/dir2/file

Test that add -s adds dirs to sparse profile

  $ sl sparse --reset
  $ sl sparse include empty
  $ sl sparse
  [include]
  empty
  [exclude]
  
  

  $ mkdir add
  $ touch add/foo
  $ touch add/bar
  $ sl add add/foo
  the following files are ignored, but still added because they are explicitly specified:
    add/foo
  (use 'sl debugignore <file>' to check why they are ignored)
  abort: cannot add 'add/foo' - it is outside the sparse checkout
  (include file with `sl sparse include <pattern>` or use `sl add -s <file>` to include file directory while adding)
  [255]
  $ sl add -s add/foo
  $ sl st
  A add/foo
  ? add/bar
  $ sl sparse
  [include]
  add
  empty
  [exclude]
  
  
  $ sl add -s add/*
  add/foo already tracked!
  $ sl st
  A add/bar
  A add/foo
  $ sl sparse
  [include]
  add
  empty
  [exclude]
  
  
Test --cwd-list
  $ sl commit -m 'commit'
  $ sl sparse --cwd-list
    add
  - hide
  - show
  - show2
  $ cd add
  $ sl sparse --cwd-list
    bar
    foo
  $ sl sparse include foo
  $ sl sparse uninclude .
  $ sl sparse show
  Additional Included Paths:
  
    add/foo
    empty
  $ sl sparse --cwd-list
  - bar
    foo

Make sure to match whole directory names, not prefixes

  $ mkdir prefix prefixpostfix
  $ touch prefix/correct prefixpostfix/incorrect
  $ sl sparse -I prefix prefixpostfix
  $ sl addremove .
  adding prefix/correct
  adding prefixpostfix/incorrect
  $ sl ci -m 'subdirs'
  $ cd prefix
  $ sl sparse --cwd-list
    correct
  $ cd ../..

  $ cd ..

Test non-sparse repos work while sparse is loaded
  $ newclientrepo nonsparserepo
  $ newclientrepo sparserepo
  $ enable sparse
  $ cd ../nonsparserepo
  $ echo x > x && sl add x && sl commit -qAm x

Test debugrebuilddirstate
  $ cd ../sparserepo
  $ touch included
  $ touch excluded
  $ sl add included excluded
  $ sl commit -m 'a commit' -q
  $ cp .sl/dirstate ../dirstateboth
  $ sl sparse -X excluded
  $ cp ../dirstateboth .sl/dirstate
  $ sl debugrebuilddirstate
  $ sl debugdirstate
  n   0         -1 unset               included

Test debugdirstate --minimal where file is in the parent manifest but not the
dirstate
  $ sl sparse -X included
  $ sl debugdirstate
  $ cp .sl/dirstate ../dirstateallexcluded
  $ sl sparse --reset
  $ sl sparse -X excluded
  $ cp ../dirstateallexcluded .sl/dirstate
  $ touch includedadded
  $ sl add includedadded
  $ sl debugdirstate --nodates
  a   0         -1 unset               includedadded
  $ sl debugrebuilddirstate --minimal
  $ sl debugdirstate --nodates
  n   0         -1 unset               included
  a   0         -1 * includedadded (glob)

Test debugdirstate --minimal where a file is not in parent manifest
but in the dirstate. This should take into account excluded files in the
manifest
  $ cp ../dirstateboth .sl/dirstate
  $ touch includedadded
  $ sl add includedadded
  $ touch excludednomanifest
  $ sl add excludednomanifest
  $ cp .sl/dirstate ../moreexcluded
  $ sl forget excludednomanifest
  $ rm excludednomanifest
  $ sl sparse -X excludednomanifest
  $ cp ../moreexcluded .sl/dirstate
  $ sl manifest
  excluded
  included
We have files in the dirstate that are included and excluded. Some are in the
manifest and some are not.
  $ sl debugdirstate --nodates
  n 644          0 * excluded (glob)
  a   0         -1 * excludednomanifest (glob)
  n 644          0 * included (glob)
  a   0         -1 * includedadded (glob)
  $ sl debugrebuilddirstate --minimal
  $ sl debugdirstate --nodates
  n 644          0 * included (glob)
  a   0         -1 * includedadded (glob)

Test logging the dirsize and sparse profiles

Set up the sampling extension and set a log file, then do a repo status.
We need to disable the SCM_SAMPLING_FILEPATH env var because arcanist may set it!

  $ touch a && sl add a
  $ unset SCM_SAMPLING_FILEPATH
  $ sl ci -m "add some new files"
  $ LOGDIR=$TESTTMP/logs
  $ mkdir $LOGDIR
  $ cat >> $HGRCPATH << EOF
  > [sampling]
  > key.dirstate_size=dirstate_size
  > key.sparse_profiles=sparse_profiles
  > filepath = $LOGDIR/samplingpath.txt
  > [extensions]
  > sampling=
  > EOF
  $ rm -f $LOGDIR/samplingpath.txt
  $ sl status
  >>> import json
  >>> with open(f'{getenv("LOGDIR")}/samplingpath.txt') as f:
  ...     data = f.read()
  >>> for record in data.strip("\0").split("\0"):
  ...     parsedrecord = json.loads(record)
  ...     if parsedrecord['category'] == 'dirstate_size':
  ...         print('{0}: {1}'.format(parsedrecord['category'],
  ...                                 parsedrecord['data']['dirstate_size']))
  dirstate_size: * (glob)
  $ cat >> profile_base << EOF
  > [include]
  > a
  > EOF
  $ cat >> profile_extended << EOF
  > %include profile_base
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [sparse]
  > largecheckouthint=True
  > largecheckoutcount=1
  > EOF
  $ sl add profile_base profile_extended
  hint[sparse-largecheckout]: Your repository checkout has * files which makes Many mercurial commands slower. Learn how to make it smaller at https://fburl.com/hgsparse (glob)
  hint[hint-ack]: use 'sl hint --ack sparse-largecheckout' to silence these hints
  $ sl ci -m 'adding sparse profiles'
  hint[sparse-largecheckout]: Your repository checkout has * files which makes Many mercurial commands slower. Learn how to make it smaller at https://fburl.com/hgsparse (glob)
  hint[hint-ack]: use 'sl hint --ack sparse-largecheckout' to silence these hints
  $ rm -f $LOGDIR/samplingpath.txt
  $ sl sparse --enable-profile profile_extended
  hint[sparse-largecheckout]: Your repository checkout has * files which makes Many mercurial commands slower. Learn how to make it smaller at https://fburl.com/hgsparse (glob)
  hint[hint-ack]: use 'sl hint --ack sparse-largecheckout' to silence these hints
  >>> import json
  >>> with open(f'{getenv("LOGDIR")}/samplingpath.txt') as f:
  ...     data = f.read()
  >>> for record in data.strip("\0").split("\0"):
  ...     parsedrecord = json.loads(record)
  ...     if parsedrecord['category'] == 'sparse_profiles':
  ...         print('active_profiles:', parsedrecord['data']['active_profiles'])
  active_profiles: %include profile_extended
  [include]
  
  [exclude]
  excluded
  excludednomanifest
  
  $ cat >> $HGRCPATH << EOF
  > [sparse]
  > largecheckouthint=False
  > EOF

Verify regular expressions are no longer supported
  $ newclientrepo rerepo
  $ enable sparse

  $ echo a > show
  $ echo x > hide
  $ cat >> sparse.profile <<EOF
  > [include]
  > re:s.ow
  > EOF
  $ sl ci -Aqm 'initial'
  $ LOG=sparse=warn sl sparse include re:sh.w
  ERROR sparse: ignoring unsupported sparse pattern err=unsupported pattern type re pat=Include("re:sh.w") src=$TESTTMP/rerepo/.sl/sparse
  $ LOG=sparse=warn sl sparse enable sparse.profile 2>&1 | head -1
  ERROR sparse: ignoring unsupported sparse pattern err=unsupported pattern type re pat=Include("re:sh.w") src=$TESTTMP/rerepo/.sl/sparse
