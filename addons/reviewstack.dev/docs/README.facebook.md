# reviewstack.dev

This folder contains the scaffolding code that takes the ReviewStack React
component in the sibling `reviewstack/` folder and hosts it in a web page so
it can be used! A version of this site is hosted publicly at
https://reviewstack.dev/.

## For Users

**tl;dr** `bunnylol reviewstack`

Currently, reviewstack.dev is public to the world, but it uses
[Netlify's password protection](https://docs.netlify.com/visitor-access/password-protection/) for authorization until public launch.
In order go get the password, you must:

- Join the [ReviewStack Feedback workplace group](https://fb.workplace.com/groups/reviewstack.feedback) to get yourself added to the GK.
- Navigate to [`bunnylol reviewstack`](https://www.internalfb.com/intern/bunny/?q=reviewstack) to see the password.

Once you have the password, head over to https://reviewstack.dev/,
click the **Authorize ReviewStack to access GitHub** to go through the OAuth
flow with GitHub, and you should be on your way!

### Examples to Try

By design, reviewstack URLs parallel GitHub URLs, so you can compare this pull
request on GitHub:

https://github.com/godot-escoria/escoria-demo-game/pull/518

by visiting this URL on reviewstack:

https://reviewstack.dev/godot-escoria/escoria-demo-game/pull/518

Note that this pull request had multiple force-pushes over its lifetime, which
helps illustrate the advanced support for stacks and versions that reviewstack
provides.

### Reporting Issues / Lavishing Praise

If you have an issue with reviewstack, please file a task against the CodeHub
project here:

https://www.internalfb.com/code/reviewstack/tasks

Or feel free to make a post in the appropriate feedback group:

https://fb.workplace.com/groups/reviewstack.feedback
https://fb.workplace.com/groups/sapling.oss

## Publishing a Release

Making a release involves doing a build and pushing the result to the
GitHub pages site for the GitHub repo.

In order to push the content live, you must have write access to
https://github.com/facebook/reviewstack. For this, you must be a member of
**Team reviewstack maintain**:

https://www.internalfb.com/intern/opensource/github/team/682346343100673/

Once you have the proper permissions, clone the repo:

```
git clone https://github.com/facebook/reviewstack ~/src/reviewstack.git
```

From the directory wit the Git clone of the repo, run the release script in
_this_ folder, e.g.:

```
~/src/reviewstack.git$ ~/fbsource/fbcode/eden/addons/reviewstack.dev/fb/push-new-version.sh
```

This will do a build using the sources in `~/fbsource/fbcode/eden/addons`,
but will push a new commit from the Git repo in `~/src/reviewstack.git`.

Sometimes it can take a minute or two for changes to propagate through GitHub's
servers, so after waiting, go to the site and verify everything is working as
intended: https://reviewstack.dev.
