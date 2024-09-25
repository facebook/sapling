  $ setconfig clone.use-rust=true

Test per-repo configs at clone time. Override "remotefilelog.reponame" since clone prints it out.
  $ cat > config.json <<EOF
  > {
  >   "hotfixes": [
  >     {
  >       "config": "\n[remotefilelog]\nreponame=override\n",
  >       "condition": {
  >         "repos": ["my-repo"]
  >       }
  >     }
  >   ]
  > }
  > EOF
  $ HGRCPATH=fb=json="$TESTTMP/config.json;$HGRCPATH" hg clone test:my-repo repo | grep override
  Cloning override into $TESTTMP/repo
