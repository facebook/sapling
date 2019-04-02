  $ . "$TESTDIR/hgsql/library.sh"
  $ initserver master masterrepo
  $ cd master

Check that a repo without an entry is treated as RW
  $ hg debugshell -c "print(repo.sqlisreporeadonly())"
  False

Set the repo to RO for hg and mononoke
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo", 0)'
  $ hg debugshell -c "print(repo.sqlisreporeadonly())"
  True

Set another repo to RW for hg and check that masterrepo is still RO
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo2", 1)'
  $ hg debugshell -c "print(repo.sqlisreporeadonly())"
  True

Set the repo to RW for Mononoke only
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo", 2)'
  $ hg debugshell -c "print(repo.sqlisreporeadonly())"
  True

Set the repo to RW for hg only
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo", 1)'
  $ hg debugshell -c "print(repo.sqlisreporeadonly())"
  False

Set the repo to RO again
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'REPLACE INTO repo_lock(repo, state) VALUES ("masterrepo", 0)'
  $ hg debugshell -c "print(repo.sqlisreporeadonly())"
  True
