# This test tries to exercise the ssh functionality with a dummy script

  $ cat <<EOF > dummyssh
  > import sys
  > import os
  > 
  > os.chdir(os.path.dirname(sys.argv[0]))
  > if sys.argv[1] != "user@dummy":
  >     sys.exit(-1)
  > 
  > if not os.path.exists("dummyssh"):
  >     sys.exit(-1)
  > 
  > log = open("dummylog", "ab")
  > log.write("Got arguments")
  > for i, arg in enumerate(sys.argv[1:]):
  >     log.write(" %d:%s" % (i+1, arg))
  > log.write("\n")
  > log.close()
  > r = os.system(sys.argv[2])
  > sys.exit(bool(r))
  > EOF

  $ checknewrepo()
  > {
  >    name=$1
  >    if [ -d $name/.hg/store ]; then
  >    echo store created
  >    fi
  >    if [ -f $name/.hg/00changelog.i ]; then
  >    echo 00changelog.i created
  >    fi
  >    cat $name/.hg/requires
  > }

creating 'local'

  $ hg init local
  $ checknewrepo local
  store created
  00changelog.i created
  revlogv1
  store
  fncache
  $ echo this > local/foo
  $ hg ci --cwd local -A -m "init" -d "1000000 0"
  adding foo

creating repo with format.usestore=false

  $ hg --config format.usestore=false init old
  $ checknewrepo old
  revlogv1

creating repo with format.usefncache=false

  $ hg --config format.usefncache=false init old2
  $ checknewrepo old2
  store created
  00changelog.i created
  revlogv1
  store

test failure

  $ hg init local
  abort: repository local already exists!

init+push to remote2

  $ hg init -e "python ./dummyssh" ssh://user@dummy/remote2
  $ hg incoming -R remote2 local
  comparing with local
  changeset:   0:c4e059d443be
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     init
  

  $ hg push -R local -e "python ./dummyssh" ssh://user@dummy/remote2
  pushing to ssh://user@dummy/remote2
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

clone to remote1

  $ hg clone -e "python ./dummyssh" local ssh://user@dummy/remote1
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

init to existing repo

  $ hg init -e "python ./dummyssh" ssh://user@dummy/remote1
  abort: repository remote1 already exists!
  abort: could not create remote repo!

clone to existing repo

  $ hg clone -e "python ./dummyssh" local ssh://user@dummy/remote1
  abort: repository remote1 already exists!
  abort: could not create remote repo!

output of dummyssh

  $ cat dummylog
  Got arguments 1:user@dummy 2:hg init remote2
  Got arguments 1:user@dummy 2:hg -R remote2 serve --stdio
  Got arguments 1:user@dummy 2:hg -R remote2 serve --stdio
  Got arguments 1:user@dummy 2:hg init remote1
  Got arguments 1:user@dummy 2:hg -R remote1 serve --stdio
  Got arguments 1:user@dummy 2:hg init remote1
  Got arguments 1:user@dummy 2:hg init remote1

comparing repositories

  $ hg tip -q -R local
  0:c4e059d443be
  $ hg tip -q -R remote1
  0:c4e059d443be
  $ hg tip -q -R remote2
  0:c4e059d443be

check names for repositories (clashes with URL schemes, special chars)

  $ for i in bundle file hg http https old-http ssh static-http " " "with space"; do
  >   printf "hg init \"$i\"... "
  >   hg init "$i"
  >   test -d "$i" -a -d "$i/.hg" && echo "ok" || echo "failed"
  > done
  hg init "bundle"... ok
  hg init "file"... ok
  hg init "hg"... ok
  hg init "http"... ok
  hg init "https"... ok
  hg init "old-http"... ok
  hg init "ssh"... ok
  hg init "static-http"... ok
  hg init " "... ok
  hg init "with space"... ok

creating 'local/sub/repo'

  $ hg init local/sub/repo
  $ checknewrepo local/sub/repo
  store created
  00changelog.i created
  revlogv1
  store
  fncache
