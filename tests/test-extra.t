Test that extra metadata (renames, copies, and other extra metadata) roundtrips
across from hg to git
  $ . "$TESTDIR/testutil"

  $ git init -q gitrepo
  $ cd gitrepo
  $ touch a
  $ git add a
  $ fn_git_commit -ma
  $ git checkout -b not-master 2>&1 | sed s/\'/\"/g
  Switched to a new branch "not-master"

  $ cd ..
  $ hg clone -q gitrepo hgrepo
  $ cd hgrepo
  $ hg mv a b
  $ fn_hg_commit -mb
  $ hg up 0 | egrep -v '^\(leaving bookmark master\)$'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch c
  $ hg add c
  $ fn_hg_commit -mc

Rebase will add a rebase_source
(The 'rebasing' is extra output in Mercurial 3.3+)

  $ hg --config extensions.rebase= rebase -s 1 -d 2 | grep -v '^rebasing '
  saved backup bundle to $TESTTMP/*.hg (glob)
  $ hg up 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

Add a commit with multiple extra fields
  $ hg bookmark b1
  $ touch d
  $ hg add d
  $ fn_hg_commitextra --field zzzzzzz=datazzz --field aaaaaaa=dataaaa
  $ hg log --graph --template "{rev} {node} {desc|firstline}\n{join(extras, ' ')}\n\n"
  @  3 f01651cfcc9337fbd9700d5018ca637a2911ed28
  |  aaaaaaa=dataaaa branch=default zzzzzzz=datazzz
  |
  o  2 03f4cf3c429050e2204fb2bda3a0f93329bdf4fd b
  |  branch=default rebase_source=4c7da7adf18b785726a7421ef0d585bb5762990d
  |
  o  1 a735dc0cd7cc0ccdbc16cfa4326b19c707c360f4 c
  |  branch=default
  |
  o  0 aa9eb6424386df2b0638fe6f480c3767fdd0e6fd a
     branch=default hg-git-rename-source=git
  
Make sure legacy extra (in commit message, after '--HG--') doesn't break
  $ hg push -r b1 --config git.debugextrainmessage=1
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 3 commits with 3 trees and 0 blobs
  adding reference refs/heads/b1

  $ hg bookmark b2
  $ hg mv c c2
  $ hg mv d d2
  $ fn_hg_commitextra --field yyyyyyy=datayyy --field bbbbbbb=databbb

Test some nutty filenames
  $ hg book b3
  $ hg mv c2 'c2 => c3'
  warning: filename contains '>', which is reserved on Windows: 'c2 => c3'
  $ fn_hg_commit -m 'test filename with arrow'
  $ hg mv 'c2 => c3' 'c3 => c4'
  warning: filename contains '>', which is reserved on Windows: 'c3 => c4'
  $ fn_hg_commit -m 'test filename with arrow 2'
  $ hg log --graph --template "{rev} {node} {desc|firstline}\n{join(extras, ' ')}\n\n" -l 3 --config "experimental.graphstyle.missing=|"
  @  6 bca4ba69a6844c133b069e227dfa043d41e3c197 test filename with arrow 2
  |  branch=default
  |
  o  5 864caad1f3493032f8d06f44a89dc9f1c039b09f test filename with arrow
  |  branch=default
  |
  o  4 58f855ae26f4930ce857e648d3dd949901cce817
  |  bbbbbbb=databbb branch=default yyyyyyy=datayyy
  |
  $ hg push -r b2 -r b3
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 3 commits with 3 trees and 0 blobs
  adding reference refs/heads/b2
  adding reference refs/heads/b3

  $ cd ../gitrepo
  $ git cat-file commit b1
  tree 1b773a2eb70f29397356f8069c285394835ff85a
  parent 202f271eb3dcb7b767ce2af6cdad4114df62ff3f
  author test <none@none> 1167609613 +0000
  committer test <none@none> 1167609613 +0000
  
  
  
  --HG--
  extra : aaaaaaa : dataaaa
  extra : zzzzzzz : datazzz

  $ git cat-file commit b2
  tree 34ad62c6d6ad9464bfe62db5b3d2fa16aaa9fa9e
  parent 66fe706f6f4f08f0020323e6c49548d41bb00ff6
  author test <none@none> 1167609614 +0000
  committer test <none@none> 1167609614 +0000
  HG:rename c:c2
  HG:rename d:d2
  HG:extra bbbbbbb:databbb
  HG:extra yyyyyyy:datayyy
  
  

  $ git cat-file commit b3
  tree e63df52695f9b06e54b37e7ef60d0c43994de620
  parent 6a66c937dea689a8bb2aa053bd91667fe4a7bfe8
  author test <none@none> 1167609616 +0000
  committer test <none@none> 1167609616 +0000
  HG:rename c2%20%3D%3E%20c3:c3%20%3D%3E%20c4
  
  test filename with arrow 2
  $ cd ../gitrepo
  $ git checkout b1
  Switched to branch 'b1'
  $ commit_sha=$(git rev-parse HEAD)
  $ tree_sha=$(git rev-parse HEAD^{tree})

There's no way to create a Git repo with extra metadata via the CLI. Dulwich
lets you do that, though.

  >>> from dulwich.objects import Commit
  >>> from dulwich.porcelain import open_repo
  >>> repo = open_repo('.')
  >>> c = Commit()
  >>> c.author = 'test <test@example.org>'
  >>> c.author_time = 0
  >>> c.author_timezone = 0
  >>> c.committer = c.author
  >>> c.commit_time = 0
  >>> c.commit_timezone = 0
  >>> c.parents = ['$commit_sha']
  >>> c.tree = '$tree_sha'
  >>> c.message = 'extra commit\n'
  >>> c.extra.extend([('zzz:zzz', 'data:zzz'), ('aaa:aaa', 'data:aaa'),
  ...                 ('HG:extra', 'hgaaa:dataaaa'),
  ...                 ('HG:extra', 'hgzzz:datazzz')])
  >>> repo.object_store.add_object(c)
  >>> repo.refs.set_if_equals('refs/heads/master', None, c.id)
  True

  $ git cat-file commit master
  tree 1b773a2eb70f29397356f8069c285394835ff85a
  parent 66fe706f6f4f08f0020323e6c49548d41bb00ff6
  author test <test@example.org> 0 +0000
  committer test <test@example.org> 0 +0000
  zzz:zzz data:zzz
  aaa:aaa data:aaa
  HG:extra hgaaa:dataaaa
  HG:extra hgzzz:datazzz
  
  extra commit

  $ cd ..
  $ hg clone -q gitrepo hgrepo2
  $ cd hgrepo2
  $ hg log --graph --template "{rev} {node} {desc|firstline}\n{join(extras, ' ')}\n\n"
  @  7 e003ec989aaae23b3eb30d4423419fb4dc346089 test filename with arrow 2
  |  branch=default
  |
  o  6 a2e276bd9458cb7dc309230ec8064d544e4f0c68 test filename with arrow
  |  branch=default
  |
  o  5 524e82e66b589f8b56bdd0679ad457a162ba16cd
  |  bbbbbbb=databbb branch=default yyyyyyy=datayyy
  |
  | o  4 741081daa02c9023c8c5117771f59ef2308a575c extra commit
  |/   GIT0-zzz%3Azzz=data%3Azzz GIT1-aaa%3Aaaa=data%3Aaaa branch=default hgaaa=dataaaa hgzzz=datazzz
  |
  o  3 73fa4063c4b0f386fd6b59da693617dedb340b02
  |  aaaaaaa=dataaaa branch=default zzzzzzz=datazzz
  |
  o  2 98337758089f6efd29f48bcaf00d14184ed0771b b
  |  branch=default rebase_source=4c7da7adf18b785726a7421ef0d585bb5762990d
  |
  o  1 92a46c8588a7cd504c369259ef631b2c14ef4e91 c
  |  branch=default hg-git-rename-source=git
  |
  o  0 aa9eb6424386df2b0638fe6f480c3767fdd0e6fd a
     branch=default hg-git-rename-source=git
  
