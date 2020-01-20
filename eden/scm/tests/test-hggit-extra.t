TODO: configure mutation
  $ configure noevolution
Test that extra metadata (renames, copies, and other extra metadata) roundtrips
across from hg to git
  $ . "$TESTDIR/hggit/testutil"

  $ git init -q gitrepo
  $ cd gitrepo
  $ touch a
  $ git add a
  $ fn_git_commit -ma
  $ git checkout -b not-master
  Switched to a new branch 'not-master'

  $ cd ..
  $ hg clone -q gitrepo hgrepo
  $ cd hgrepo
  $ hg mv a b
  $ fn_hg_commit -mb
  $ hg up 0 | egrep -v '^\(leaving bookmark'
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
  @  3 f85ec44c632713d6fa812de1bf18a530e1dd6551
  |  aaaaaaa=dataaaa branch=default zzzzzzz=datazzz
  |
  o  2 f3c80cf66a137d4862c82e4df65a7c952aad36af b
  |  branch=default rebase_source=26b80d272c9a2d4455e269005f4f250adc4c05b8
  |
  o  1 907635da058b4bd98a9594843a3bb7c61baed082 c
  |  branch=default
  |
  o  0 5b699970cd13b5f95f6af5f32781d80cfa2e813b a
     branch=default convert_revision=ad4fd0de4cb839a7d2d1c2497f8a2c230a2726e9 hg-git-rename-source=git
  
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
  @  6 decbc0c79131e24c0f01a480f068af7f0957872e test filename with arrow 2
  |  branch=default
  |
  o  5 30e7f0dfaf1aa9bc81fe995415900b76021df952 test filename with arrow
  |  branch=default
  |
  o  4 256d56838c39ba60599eb69373038b88403ba2e4
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
  parent 2ab6104c03f0d6e0885470a7cc1bcd9b26f70bad
  author test <none@none> 1167609613 +0000
  committer test <none@none> 1167609613 +0000
  
  
  
  --HG--
  extra : aaaaaaa : dataaaa
  extra : zzzzzzz : datazzz

  $ git cat-file commit b2
  tree 34ad62c6d6ad9464bfe62db5b3d2fa16aaa9fa9e
  parent f554f3e7146694b2197fd3c853eef527ba264ae7
  author test <none@none> 1167609614 +0000
  committer test <none@none> 1167609614 +0000
  HG:rename c:c2
  HG:rename d:d2
  HG:extra bbbbbbb:databbb
  HG:extra yyyyyyy:datayyy
  
  

  $ git cat-file commit b3
  tree e63df52695f9b06e54b37e7ef60d0c43994de620
  parent e16d81cc6d51456f445ddcd159b25361473d659c
  author test <none@none> 1167609616 +0000
  committer test <none@none> 1167609616 +0000
  HG:rename c2%20%3D%3E%20c3:c3%20%3D%3E%20c4
  
  test filename with arrow 2
  $ cd ../gitrepo
  $ git checkout b1
  Switched to branch 'b1'
  $ commit_sha=`git rev-parse HEAD`
  $ tree_sha=`git rev-parse 'HEAD^{tree}'`

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
  parent f554f3e7146694b2197fd3c853eef527ba264ae7
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
  @  7 193fc708cfa16eb942a0dde3017d52c6743a43ae test filename with arrow 2
  |  branch=default convert_revision=fb079f663e17f780a63855d7116b0b4f867b2371
  |
  o  6 1cc3fc4cf203075484b5dbb141d4eb91bd205dc1 test filename with arrow
  |  branch=default convert_revision=e16d81cc6d51456f445ddcd159b25361473d659c
  |
  o  5 f1aecb2ae22f40a9369287f87d5f987eeae1f25e
  |  bbbbbbb=databbb branch=default convert_revision=acd860f8f036a235465c7d5e003ce9f28383b5f2 yyyyyyy=datayyy
  |
  | o  4 0d9e73e512aa715e73f9f37be9c4dec4224d2615 extra commit
  |/   GIT0-zzz%3Azzz=data%3Azzz GIT1-aaa%3Aaaa=data%3Aaaa branch=default convert_revision=0f7316e5c44bf7af9199e8c728938ba3daf058cb hgaaa=dataaaa hgzzz=datazzz
  |
  o  3 f9541591947764cf1c54ec8331b0618b710807bc
  |  aaaaaaa=dataaaa branch=default convert_revision=f554f3e7146694b2197fd3c853eef527ba264ae7 zzzzzzz=datazzz
  |
  o  2 4e11085eb947c77f6de15ee7a64d2752cb12b399 b
  |  branch=default convert_revision=2ab6104c03f0d6e0885470a7cc1bcd9b26f70bad rebase_source=26b80d272c9a2d4455e269005f4f250adc4c05b8
  |
  o  1 1ec8735a89979fc3cb5e8edf1c03d6b61de3176b c
  |  branch=default convert_revision=8728d16f575a12b85c99ddf5763972c3740515d9 hg-git-rename-source=git
  |
  o  0 5b699970cd13b5f95f6af5f32781d80cfa2e813b a
     branch=default convert_revision=ad4fd0de4cb839a7d2d1c2497f8a2c230a2726e9 hg-git-rename-source=git
  
