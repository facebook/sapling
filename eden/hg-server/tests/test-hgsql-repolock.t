  $ . "$TESTDIR/hgsql/library.sh"
  $ initserver master masterrepo
  $ cd master

Check that a repo without an entry is treated as RW
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (False, 'no reason was provided')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  False

Set the repo to RO for hg and mononoke
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo", 0)'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (True, 'no reason was provided')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  True

Set another repo to RW for hg and check that masterrepo is still RO
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo2", 1)'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (True, 'no reason was provided')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  True

Set the repo to RW for Mononoke only
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo", 2)'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (True, 'writes are being served by Mononoke (fburl.com/mononoke)')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  True

Set the repo to RW for hg only
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo", 1)'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (False, 'no reason was provided')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  False

Set the repo to RO again
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo", 0)'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (True, 'no reason was provided')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  True
Set the repo to RO for hg and mononoke (with reason)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state, reason) VALUES ("masterrepo", 0, "reason123")'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (True, 'reason123')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  True

Set another repo to RW (with reason) for hg and check that masterrepo is still RO
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state, reason) VALUES ("masterrepo2", 1, "reason456")'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (True, 'reason123')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  True

Set the repo to RW for Mononoke only (with reason)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state, reason) VALUES ("masterrepo", 2, "reason123")'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (True, 'reason123')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  True

Set the repo to RW for hg only (with reason)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state, reason) VALUES ("masterrepo", 1, "reason123")'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (False, 'reason123')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  False

Set the repo to RO again (with reason)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state, reason) VALUES ("masterrepo", 0, "reason123")'
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlreporeadonlystate()))"
  (True, 'reason123')
  $ hg debugshell -c "ui.write('%s\n' % str(repo.sqlisreporeadonly()))"
  True
