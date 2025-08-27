#require eden

  $ setconfig clone.eden-preferred-destination-regex=.*good-name

  $ hg clone test:server bad-name
  Cloning server into $TESTTMP/bad-name
  WARNING: Clone destination $TESTTMP/bad-name is not a preferred location and may result in a bad experience.
           Run 'hg config clone.eden-preferred-destination-regex' to see the preferred location regex.
  Server has no 'master' bookmark - trying tip.

  $ hg clone test:server good-name
  Cloning server into $TESTTMP/good-name
  Server has no 'master' bookmark - trying tip.
