  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!

Test operations on server repository with bad configuration fail in expected
ways.

  $ hg init master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > globalrevs=
  > [hgsql]
  > enabled = True
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > [globalrevs]
  > reponame = customname
  > EOF

- Expectation is to fail because hgsql extension is not enabled.

  $ hg log -r 'tip' -T {node}
  abort: hgsql extension is not enabled
  [255]


- Properly configure the server with respect to hgsql extension.

  $ configureserver . master


- Expectation is to fail because pushrebase extension is not enabled.

  $ hg log -r 'tip' -T {node}
  abort: pushrebase extension is not enabled
  [255]


- Enable pushrebase extension on the server.

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF


- Expectation is to fail because we need to configure pushrebase to only allow
commits created through pushrebase extension.

  $ hg log -r 'tip' -T {node}
  abort: pushrebase using incorrect configuration
  [255]


- We can override the option to allow creation of commits only through
pushrebase by setting `globalrevs.onlypushrebase` as False which will make the
previous command succeed as we won't care about the pushrebase configuration.

  $ hg log -r 'tip' -T {node} --config globalrevs.onlypushrebase=False
  0000000000000000000000000000000000000000 (no-eol)


- Configure server repository to only allow commits created through pushrebase.

  $ cat >> .hg/hgrc <<EOF
  > [pushrebase]
  > blocknonpushrebase = True
  > EOF


- Test that the `globalrev` command fails because there is no entry in the
database for the next available strictly increasing revision number.

  $ hg globalrev
  abort: no commit counters for customname in database
  [255]


- Test that the `initglobalrev` command fails when run without the
`--i-know-what-i-am-doing` flag.

  $ hg initglobalrev 5000
  abort: * (glob)
  [255]


- Test that incorrect arguments to the `initglobalrev` command result in error.

  $ hg initglobalrev "blah" --i-know-what-i-am-doing
  abort: start must be an integer.
  [255]


- Configure the next available strictly increasing revision number to be 5000.
  $ hg initglobalrev 5000 --i-know-what-i-am-doing
  $ hg globalrev
  5000


- Check that we can only set the next available strictly increasing revision
number once.

  $ hg initglobalrev 5000 --i-know-what-i-am-doing 2> /dev/null
  [1]


- Server is configured properly now. We can create an initial commit in the
database.

  $ hg log -r 'tip' -T {node}
  0000000000000000000000000000000000000000 (no-eol)

  $ touch a && hg ci -Aqm a --config extensions.globalrevs=!
  $ hg book master


Test that pushing to a server with the `globalrevs` extension enabled leads to
creation of commits with strictly increasing revision numbers accessible through
the `globalrev` template.

- Configure client. `globalrevs` extension is enabled for making the `globalrev`
template available to the client.

  $ cd ..
  $ initclient client
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > globalrevs=
  > pushrebase=
  > [experimental]
  > evolution = all
  > EOF


- Make commits on the client.

  $ hg pull -q ssh://user@dummy/master
  $ hg up -q tip
  $ touch b && hg ci -Aqm b
  $ touch c && hg ci -Aqm c


- Finally, push the commits to the server.

  $ hg push -q ssh://user@dummy/master --to master


- Check that the `globalrev` template on the client and server shows strictly
increasing revision numbers for the pushed commits.

  $ hg log -GT '{globalrev} {desc}\n'
  @  5001 c
  |
  o  5000 b
  |
  o   a
  

  $ cd ../master
  $ hg log -GT '{globalrev} {desc}\n'
  o  5001 c
  |
  o  5000 b
  |
  @   a
  


Test that running the `globalrev` command on the client fails.

  $ cd ../client
  $ hg globalrev
  abort: this repository is not a sql backed repository
  [255]

  $ cd ../master


Test that failure of the transaction is handled gracefully and does not affect
the assignment of subsequent strictly increasing revision numbers.

- Configure the transaction to always fail before closing on the server.

  $ cp .hg/hgrc .hg/hgrc.bak
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > pretxnclose.error = exit 1
  > EOF


- Make some commits on the client.

  $ cd ../client
  $ touch d && hg ci -Aqm d
  $ touch e && hg ci -Aqm e


- Try pushing the commits to the server. Push should fail because of the
incorrect configuration on the server.

  $ hg push -q ssh://user@dummy/master --to master
  abort: push failed on remote
  [255]


- Fix the configuration on the server and retry. This time the pushing should
succeed.

  $ cd ../master
  $ mv .hg/hgrc.bak .hg/hgrc

  $ cd ../client
  $ hg push -q ssh://user@dummy/master --to master


- Check that both the client and server have the expected strictly increasing
revisions numbers.

  $ hg log -GT '{globalrev} {desc}\n'
  @  5003 e
  |
  o  5002 d
  |
  o  5001 c
  |
  o  5000 b
  |
  o   a
  

  $ cd ../master
  $ hg log -GT '{globalrev} {desc}\n'
  o  5003 e
  |
  o  5002 d
  |
  o  5001 c
  |
  o  5000 b
  |
  @   a
  


Test pushing to a different head on the server.

- Make some commits on the client to a different head (other than the current
tip).

  $ cd ../client
  $ hg up -q 'tip^'
  $ touch f && hg ci -Aqm f
  $ touch g && hg ci -Aqm g


- Push the commits to the server.

  $ hg push -q ssh://user@dummy/master --to master


- Check that both the client and server have the expected strictly increasing
revisions numbers.

  $ hg log -GT '{globalrev} {desc}\n'
  @  5005 g
  |
  o  5004 f
  |
  | o  5003 e
  |/
  o  5002 d
  |
  o  5001 c
  |
  o  5000 b
  |
  o   a
  

  $ cd ../master
  $ hg log -GT '{globalrev} {desc}\n'
  o  5005 g
  |
  o  5004 f
  |
  | o  5003 e
  |/
  o  5002 d
  |
  o  5001 c
  |
  o  5000 b
  |
  @   a
  


Test cherry picking commits from a branch and pushing to another branch.

- On the client, cherry pick a commit from one branch to copy to the only other
branch head. In particular, we are copying the commit with description `g` on
top of commit with description `e`.

  $ cd ../client
  $ hg rebase -qk -d 'desc("e")' -r 'tip' --collapse -m g1 \
  > --config extensions.rebase=

- Check that the rebase did not add `globalrev` to the commit since the commit
did not reach the server yet.

  $ hg log -GT '{globalrev} {desc}\n'
  @   g1
  |
  | o  5005 g
  | |
  | o  5004 f
  | |
  o |  5003 e
  |/
  o  5002 d
  |
  o  5001 c
  |
  o  5000 b
  |
  o   a
  

- Push the commits to the server.

  $ hg push -q ssh://user@dummy/master --to master


- Check that both the client and server have the expected strictly increasing
revisions numbers.

  $ hg log -GT '{globalrev} {desc}\n'
  @  5006 g1
  |
  | o  5005 g
  | |
  | o  5004 f
  | |
  o |  5003 e
  |/
  o  5002 d
  |
  o  5001 c
  |
  o  5000 b
  |
  o   a
  

  $ cd ../master
  $ hg log -GT '{globalrev} {desc}\n'
  o  5006 g1
  |
  | o  5005 g
  | |
  | o  5004 f
  | |
  o |  5003 e
  |/
  o  5002 d
  |
  o  5001 c
  |
  o  5000 b
  |
  @   a
  


Test simultaneous pushes to different heads.

- Configure the existing server to not work on incoming changegroup immediately.

  $ cp .hg/hgrc .hg/hgrc.bak
  $ printf "[hooks]\npre-changegroup.sleep = sleep 2\n" >> .hg/hgrc


- Create a second server.

  $ cd ..
  $ initserver master2 master

  $ cd master2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > globalrevs=
  > pushrebase=
  > [pushrebase]
  > blocknonpushrebase=True
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > [globalrevs]
  > reponame = customname
  > EOF


- Create a second client corresponding to the second server.

  $ cd ..
  $ initclient client2

  $ hg pull -q -R client2 ssh://user@dummy/master2

  $ cd client2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > globalrevs=
  > pushrebase=
  > [experimental]
  > evolution = all
  > EOF


- Make some commits on top of the tip commit on the first client.

  $ cd ../client
  $ hg up -q 'tip'
  $ touch h1 && hg ci -Aqm h1
  $ touch i && hg ci -Aqm i


- Make some commits on top of the tip commit on the second client.

  $ cd ../client2
  $ hg up -q 'tip'
  $ touch h2 && hg ci -Aqm h2


- Push the commits from both the clients.

  $ cd ..
  $ hg push -R client -q ssh://user@dummy/master --to master &
  $ hg push -R client2 -q -f ssh://user@dummy/master2 --to master


- Introduce some bash functions to help with testing

  $ getglobalrev()
  > {
  >   echo `hg log -r "$1" -T "{globalrev}"`
  > }

  $ isgreaterglobalrev()
  > {
  >   [ `getglobalrev "$1"` -gt `getglobalrev "$2"` ]
  > }

  $ isnotequalglobalrev()
  > {
  >   [ `getglobalrev "$1"` -ne `getglobalrev "$2"` ]
  > }

  $ checkglobalrevs()
  > {
  >   isgreaterglobalrev 'desc("h2")' 'desc("g1")' && \
  >   isgreaterglobalrev 'desc("i")' 'desc("h1")' && \
  >   isgreaterglobalrev 'desc("h1")' 'desc("g1")' && \
  >   isnotequalglobalrev 'desc("i")' 'desc("h2")' && \
  >   isnotequalglobalrev 'desc("h1")' 'desc("h2")'
  > }


- Check that both the servers have the expected strictly increasing revision
numbers.

  $ cd master
  $ checkglobalrevs

  $ cd ../master2
  $ checkglobalrevs


- Check that both the clients have the expected strictly increasing revisions
numbers.

  $ cd ../client
  $ isgreaterglobalrev 'desc("i")' 'desc("h1")'
  $ isgreaterglobalrev 'desc("h1")' 'desc("g1")'

  $ cd ../client2
  $ isgreaterglobalrev 'desc("h2")' 'desc("g1")'


- Check that the clients have the expected strictly increasing revision numbers
after a pull.

  $ cd ../client
  $ hg pull -q ssh://user@dummy/master
  $ checkglobalrevs

  $ cd ../client2
  $ hg pull -q ssh://user@dummy/master2
  $ checkglobalrevs


Test resolving commits based on the strictly increasing global revision numbers.

- Test that incorrect lookups result in errors.

  $ cd ../client

  $ hg log -r 'globalrev()'
  hg: parse error: globalrev takes one argument
  [255]

  $ hg log -r 'globalrev(1, 2)'
  hg: parse error: globalrev takes one argument
  [255]

  $ hg log -r 'globalrev(invalid_input_type)'
  hg: parse error: the argument to globalrev() must be a number
  [255]

  $ hg log -r 'munknown'
  abort: unknown revision 'munknown'!
  (if munknown is a remote bookmark or commit, try to 'hg pull' it first)
  [255]


- Test that correct lookups work as expected.

  $ testlookup()
  > {
  >   local grevs=$(hg log -r 'all()' -T '{globalrev}\n' | sed '/^$/d')
  >   # There should be at least one globalrev based commit otherwise something
  >   # is wrong.
  >   if [ -z "$grevs" ]; then
  >      return 1
  >   fi
  >   for grev in $grevs; do
  >     if [ "${grev}" -ne `getglobalrev "globalrev(${grev})"` ] || \
  >       [ "${grev}" -ne `getglobalrev "m${grev}"` ]; then
  >       return 1
  >     fi
  >   done
  > }

  $ testlookup


- Test that non existent global revision numbers do not resolve to any commit in
the repository. In particular, lets test fetching the commit corresponding to
global revision number 4999 which should not exist as the counting starts from
5000 in our test cases.

  $ hg log -r 'globalrev(4999)'

  $ hg log -r 'm4999'
  abort: unknown revision 'm4999'!
  (if m4999 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

  $ hg log -r 'm1+m2'
  abort: unknown revision 'm1'!
  (if m1 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

  $ hg log -r 'globalrev(-1)'


- Creating a bookmark with prefix `m1` should still work.

  $ hg bookmark -r null m1
  $ hg log -r 'm1' -T '{node}\n'
  0000000000000000000000000000000000000000


- Test globalrevs extension with read only configuration on the first server.

- Configure the first server to have read only mode for globalrevs extension.

  $ cd ../master
  $ cp .hg/hgrc.bak .hg/hgrc
  $ cat >> .hg/hgrc <<EOF
  > [globalrevs]
  > readonly = True
  > EOF


- Queries not involving writing data to commits should still work.

  $ testlookup


Test bypassing hgsql extension on the first server.

- Configure the first server to bypass hgsql extension.

  $ mv .hg/hgrc.bak .hg/hgrc
  $ cat >> .hg/hgrc <<EOF
  > [hgsql]
  > bypass = True
  > EOF


- Queries not involving the hgsql extension should still work.

  $ testlookup


Test that the global revisions are only effective beyond the `startrev`
configuration in the globalrevs extension.

- Helper function to get the globalrev for the first globalrev based commit.

  $ firstvalidglobalrevcommit()
  > {
  >   local -i startrev=$1
  >   hg log -r 'all()' \
  >   --config globalrevs.startrev="$startrev" \
  >   -T '{globalrev}\n'  \
  >   | sed '/^$/d' \
  >   | head -1
  > }


- If the `startrev` is less than the first globalrev based commit i.e. 5000 then
effectively all globalrevs based commits in the repository have valid global
revision numbers.

  $ firstvalidglobalrevcommit 4999
  5000


- If the `startrev` is equal to the first globalrev based commit i.e. 5000 then
effectively all globalrevs based commits in the repository have valid global
revision numbers.

  $ firstvalidglobalrevcommit 5000
  5000


- If the `startrev` is greater than the first globalrev based commit i.e. 5000
then effectively only the globalrevs based commit in the repository >=
`startrev` have valid global revision numbers.

  $ firstvalidglobalrevcommit 5003
  5003


- If the `startrev` is greater than the last globalrev based commit i.e. 5009
then there is no commit which has a valid global revision number in the
repository.

  $ firstvalidglobalrevcommit 5010


- Configure the repository with `startrev` as 5005.

  $ cat >> .hg/hgrc <<EOF
  > [globalrevs]
  > startrev = 5005
  > EOF


- Test that lookup works for commits with  globalrev >= `startrev`.

  $ getglobalrev 'globalrev(5006)'
  5006

  $ getglobalrev 'm5005'
  5005


- Test that lookup fails for commits with globalrev < `startrev`.

  $ getglobalrev 'globalrev(5003)'
  

  $ getglobalrev 'm5004'
  abort: unknown revision 'm5004'!
  (if m5004 is a remote bookmark or commit, try to 'hg pull' it first)
  


- Test that the lookup works as expected when the configuration
`globalrevs.fastlookup` is true.

  $ cd ../client
  $ setconfig globalrevs.fastlookup=True

  $ testlookup

  $ getglobalrev 'globalrev(4999)'
  

  $ getglobalrev 'globalrev(-1)'
  

  $ hg updateglobalrevmeta

  $ testlookup

  $ getglobalrev 'globalrev(4999)'
  

  $ getglobalrev 'globalrev(-1)'
  
