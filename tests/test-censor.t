  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > censor=
  > EOF
  $ cp $HGRCPATH $HGRCPATH.orig

Create repo with unimpeachable content

  $ hg init r
  $ cd r
  $ echo 'Initially untainted file' > target
  $ echo 'Normal file here' > bystander
  $ hg add target bystander
  $ hg ci -m init

Clone repo so we can test pull later

  $ cd ..
  $ hg clone r rpull
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd r

Introduce content which will ultimately require censorship. Name the first
censored node C1, second C2, and so on

  $ echo 'Tainted file' > target
  $ echo 'Passwords: hunter2' >> target
  $ hg ci -m taint target
  $ C1=`hg id --debug -i`

  $ echo 'hunter3' >> target
  $ echo 'Normal file v2' > bystander
  $ hg ci -m moretaint target bystander
  $ C2=`hg id --debug -i`

Add a new sanitized versions to correct our mistake. Name the first head H1,
the second head H2, and so on

  $ echo 'Tainted file is now sanitized' > target
  $ hg ci -m sanitized target
  $ H1=`hg id --debug -i`

  $ hg update -r $C2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'Tainted file now super sanitized' > target
  $ hg ci -m 'super sanitized' target
  created new head
  $ H2=`hg id --debug -i`

Verify target contents before censorship at each revision

  $ hg cat -r 3 target
  Tainted file is now sanitized
  $ hg cat -r $H2 target
  Tainted file now super sanitized
  $ hg cat -r $C2 target
  Tainted file
  Passwords: hunter2
  hunter3
  $ hg cat -r $C1 target
  Tainted file
  Passwords: hunter2
  $ hg cat -r 0 target
  Initially untainted file

Try to censor revision with too large of a tombstone message

  $ hg censor -r $C1 -t 'blah blah blah blah blah blah blah blah bla' target
  abort: censor tombstone must be no longer than censored data
  [255]

Censor revision with 2 offenses

  $ hg censor -r $C2 -t "remove password" target
  $ hg cat -r 3 target
  Tainted file is now sanitized
  $ hg cat -r $H2 target
  Tainted file now super sanitized
  $ hg cat -r $C2 target
  abort: censored node: 1e0247a9a4b7
  (set censor.policy to ignore errors)
  [255]
  $ hg cat -r $C1 target
  Tainted file
  Passwords: hunter2
  $ hg cat -r 0 target
  Initially untainted file

Censor revision with 1 offense

  $ hg censor -r $C1 target
  $ hg cat -r 3 target
  Tainted file is now sanitized
  $ hg cat -r $H2 target
  Tainted file now super sanitized
  $ hg cat -r $C2 target
  abort: censored node: 1e0247a9a4b7
  (set censor.policy to ignore errors)
  [255]
  $ hg cat -r $C1 target
  abort: censored node: 613bc869fceb
  (set censor.policy to ignore errors)
  [255]
  $ hg cat -r 0 target
  Initially untainted file

Can only checkout target at uncensored revisions, -X is workaround for --all

  $ hg revert -r $C2 target
  abort: censored node: 1e0247a9a4b7
  (set censor.policy to ignore errors)
  [255]
  $ hg revert -r $C1 target
  abort: censored node: 613bc869fceb
  (set censor.policy to ignore errors)
  [255]
  $ hg revert -r $C1 --all
  reverting bystander
  reverting target
  abort: censored node: 613bc869fceb
  (set censor.policy to ignore errors)
  [255]
  $ hg revert -r $C1 --all -X target
  $ cat target
  Tainted file now super sanitized
  $ hg revert -r 0 --all
  reverting target
  $ cat target
  Initially untainted file
  $ hg revert -r $H2 --all
  reverting bystander
  reverting target
  $ cat target
  Tainted file now super sanitized

Uncensored file can be viewed at any revision

  $ hg cat -r 3 bystander
  Normal file v2
  $ hg cat -r $C2 bystander
  Normal file v2
  $ hg cat -r $C1 bystander
  Normal file here
  $ hg cat -r 0 bystander
  Normal file here

Can update to children of censored revision

  $ hg update -r 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat target
  Tainted file is now sanitized
  $ hg update -r $H2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat target
  Tainted file now super sanitized

Set censor policy to abort in trusted $HGRC so hg verify fails

  $ cp $HGRCPATH.orig $HGRCPATH
  $ cat >> $HGRCPATH <<EOF
  > [censor]
  > policy = abort
  > EOF

Repo fails verification due to censorship

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   target@1: censored file data
   target@2: censored file data
  2 files, 5 changesets, 7 total revisions
  2 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]

Cannot update to revision with censored data

  $ hg update -r $C2
  abort: censored node: 1e0247a9a4b7
  (set censor.policy to ignore errors)
  [255]
  $ hg update -r $C1
  abort: censored node: 613bc869fceb
  (set censor.policy to ignore errors)
  [255]
  $ hg update -r 0
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg update -r $H2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Set censor policy to ignore in trusted $HGRC so hg verify passes

  $ cp $HGRCPATH.orig $HGRCPATH
  $ cat >> $HGRCPATH <<EOF
  > [censor]
  > policy = ignore
  > EOF

Repo passes verification with warnings with explicit config

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 5 changesets, 7 total revisions

May update to revision with censored data with explicit config

  $ hg update -r $C2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat target
  $ hg update -r $C1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat target
  $ hg update -r 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat target
  Initially untainted file
  $ hg update -r $H2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat target
  Tainted file now super sanitized

Can merge in revision with censored data. Test requires one branch of history
with the file censored, but we can't censor at a head, so advance H1.

  $ hg update -r $H1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ C3=$H1
  $ echo 'advanced head H1' > target
  $ hg ci -m 'advance head H1' target
  $ H1=`hg id --debug -i`
  $ hg censor -r $C3 target
  $ hg update -r $H2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge -r $C3
  merging target
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Revisions present in repository heads may not be censored

  $ hg update -C -r $H2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg censor -r $H2 target
  abort: cannot censor file in heads (78a8fc215e79)
  (clean/delete and commit first)
  [255]
  $ echo 'twiddling thumbs' > bystander
  $ hg ci -m 'bystander commit'
  $ H2=`hg id --debug -i`
  $ hg censor -r "$H2^" target
  abort: cannot censor file in heads (efbe78065929)
  (clean/delete and commit first)
  [255]

Cannot censor working directory

  $ echo 'seriously no passwords' > target
  $ hg ci -m 'extend second head arbitrarily' target
  $ H2=`hg id --debug -i`
  $ hg update -r "$H2^"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg censor -r . target
  abort: cannot censor working directory
  (clean/delete/update first)
  [255]
  $ hg update -r $H2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Can re-add file after being deleted + censored

  $ C4=$H2
  $ hg rm target
  $ hg ci -m 'delete target so it may be censored'
  $ H2=`hg id --debug -i`
  $ hg censor -r $C4 target
  $ hg cat -r $C4 target
  $ hg cat -r "$H2^^" target
  Tainted file now super sanitized
  $ echo 'fresh start' > target
  $ hg add target
  $ hg ci -m reincarnated target
  $ H2=`hg id --debug -i`
  $ hg cat -r $H2 target
  fresh start
  $ hg cat -r "$H2^" target
  target: no such file in rev 452ec1762369
  [1]
  $ hg cat -r $C4 target
  $ hg cat -r "$H2^^^" target
  Tainted file now super sanitized

Can censor after revlog has expanded to no longer permit inline storage

  $ for x in `seq 0 50000`
  > do
  >   echo "Password: hunter$x" >> target
  > done
  $ hg ci -m 'add 100k passwords'
  $ H2=`hg id --debug -i`
  $ C5=$H2
  $ hg revert -r "$H2^" target
  $ hg ci -m 'cleaned 100k passwords'
  $ H2=`hg id --debug -i`
  $ hg censor -r $C5 target
  $ hg cat -r $C5 target
  $ hg cat -r $H2 target
  fresh start
