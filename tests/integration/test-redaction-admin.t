  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

  $ cd $TESTTMP

setup hg server repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg ci -A -q -m 'add a'

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-pull and repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate

blobimport
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > rebase =
  > remotenames =
  > EOF

  $ cd ../repo-push

  $ hgmn up -q 0
Push files
  $ echo b > b
  $ echo f > f

  $ mkdir dir
  $ mkdir dir/dirdir
  $ echo 'c' > dir/c
  $ echo 'd' > dir/d
  $ echo 'g' > dir/g
  $ echo 'e' > dir/dirdir/e
  $ hg ci -A -q -m "add b,c,d and e"

  $ hgmn push -q -r .  --to master_bookmark

  $ tglogpnr
  @  2cc2702dde1d public 'add b,c,d and e'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

Censor file (file 'b' in commit '2cc2702dde1d7133c30a1ed763ee82c04befb237')
  $ mononoke_admin redaction add "[TASK]Censor b" 2cc2702dde1d7133c30a1ed763ee82c04befb237 b
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT * FROM censored_contents;'
  1|content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9|[TASK]Censor b|* (glob)

Censor file inside directory (file 'dir/c' in commit '2cc2702dde1d7133c30a1ed763ee82c04befb237')
  $ mononoke_admin redaction add "[TASK]Censor c" 2cc2702dde1d7133c30a1ed763ee82c04befb237 dir/c
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT * FROM censored_contents;'
  1|content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9|[TASK]Censor b|* (glob)
  2|content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2|[TASK]Censor c|* (glob)

Censor multiple files
  $ mononoke_admin redaction add "[TASK]Censor g,f" 2cc2702dde1d7133c30a1ed763ee82c04befb237 f dir/g
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT * FROM censored_contents;'
  1|content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9|[TASK]Censor b|* (glob)
  2|content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2|[TASK]Censor c|* (glob)
  3|content.blake2.5119c9ed8ede459c6992624164307f82dc1edc3efd074481a4cc9afdb7755061|[TASK]Censor g,f|* (glob)
  4|content.blake2.0991063aafe55b2bcbbfa6b349e76ab5d57a102c89e841abdac8ce3f84d55b8a|[TASK]Censor g,f|* (glob)

Expect error when censoring tree
  $ mononoke_admin redaction add "[TASK]Censor dir" 2cc2702dde1d7133c30a1ed763ee82c04befb237 dir/dirdir
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * failed to identify the files associated with the file paths [MPath("dir/dirdir")] (glob)
  [1]

Expect error when trying to censor nonexisting file
  $ mononoke_admin redaction add "[TASK]Censor nofile" 2cc2702dde1d7133c30a1ed763ee82c04befb237 dir/dirdir/nofile
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * failed to identify the files associated with the file paths [MPath("dir/dirdir/nofile")] (glob)
  [1]

No new entry in the table
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT * FROM censored_contents;'
  1|content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9|[TASK]Censor b|* (glob)
  2|content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2|[TASK]Censor c|* (glob)
  3|content.blake2.5119c9ed8ede459c6992624164307f82dc1edc3efd074481a4cc9afdb7755061|[TASK]Censor g,f|* (glob)
  4|content.blake2.0991063aafe55b2bcbbfa6b349e76ab5d57a102c89e841abdac8ce3f84d55b8a|[TASK]Censor g,f|* (glob)

Uncensor some of the stuff
  $ mononoke_admin redaction remove 2cc2702dde1d7133c30a1ed763ee82c04befb237 f dir/g
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)

Fewer entries in the table
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT * FROM censored_contents;'
  1|content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9|[TASK]Censor b|* (glob)
  2|content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2|[TASK]Censor c|* (glob)

Let's make sure multiple files can be blacklisted under the same task
  $ mononoke_admin redaction add "[TASK]Censor b" 2cc2702dde1d7133c30a1ed763ee82c04befb237 dir/g
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)

List blacklisted files:
  $ mononoke_admin redaction list 2cc2702dde1d7133c30a1ed763ee82c04befb237
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Listing blacklisted files for ChangesetId: HgChangesetId(HgNodeHash(Sha1(2cc2702dde1d7133c30a1ed763ee82c04befb237))) (glob)
  * Please be patient. (glob)
  * [TASK]Censor b      : b (glob)
  * [TASK]Censor b      : dir/g (glob)
  * [TASK]Censor c      : dir/c (glob)

List blacklisted files for a commit without any
  $ mononoke_admin redaction list ac82d8b1f7c418c61a493ed229ffaa981bda8e90
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Listing blacklisted files for ChangesetId: HgChangesetId(HgNodeHash(Sha1(ac82d8b1f7c418c61a493ed229ffaa981bda8e90))) (glob)
  * Please be patient. (glob)
  * No files are blacklisted at this commit (glob)
