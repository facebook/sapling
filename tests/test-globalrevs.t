#testcases case-innodb case-rocksdb

#if case-rocksdb
  $ DBENGINE=rocksdb
#else
  $ DBENGINE=innodb
#endif

  $ . "$TESTDIR/hgsql/library.sh"

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


- Configure server repository to only allow commits created through pushrebase.

  $ cat >> .hg/hgrc <<EOF
  > [pushrebase]
  > blocknonpushrebase = True
  > EOF


- Test that the `globalrev` command fails because there is no entry in the
database for the next available strictly increasing revision number.

  $ hg globalrev
  abort: no commit counters for master in database
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

  $ touch a && hg ci -Aqm a
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


Test errors for bad configuration of the hgsql extension on the first server

- Configure the first server to bypass hgsql extension.

  $ cd ../master
  $ mv .hg/hgrc.bak .hg/hgrc
  $ cat >> .hg/hgrc <<EOF
  > [hgsql]
  > bypass = True
  > EOF


- Make a commit on the first client.

  $ cd ../client
  $ touch j && hg ci -Aqm j


- Try to push the commit to the first server. It should fail because the hgsql
extension is misconfigured.

  $ hg push ssh://user@dummy/master --to master
  pushing to ssh://user@dummy/master
  remote: abort: hgsql using incorrect configuration
  abort: no suitable response from remote hg!
  [255]
