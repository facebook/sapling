# ReviewStack

CodeHub Project: https://www.internalfb.com/code/reviewstack

Feedback Group: https://fb.workplace.com/groups/reviewstack.feedback

## Local Development

ReviewStack must be done locally because it requires running a web server.
When creating your local checkout, you have two options:

### Non-EdenFS with a Sparse Profile

As there are still some issues with EdenFS on macOS that may cause performance
issues during development, a straightforward alternative is to clone
fbsource with a _sparse profile_:

```sh
$ fbclone fbsource --sparse tools/scm/sparse/base/base ~/reviewstack
$ cd ~/reviewstack
$ hg sparse include fbcode/eden/addons/
$ hg sparse include xplat/third-party/yarn/
$ hg sparse include xplat/third-party/node/
```

Note that we clone into `~/reviewstack` instead of `~/fbsource` to
remind us that this is not a full checkout.

Note that you should also:

- Go to **System Preferences** -> **Spotlight** and add `~/reviewstack` to the
  **Privacy** list so that Spotlight does not try to index it.
- Run `eden stop` if you are not going to be using EdenFS for other checkouts.
  (Note that EdenFS will start up again upon reboot.)

### Using EdenFS

This will clone fbsource into `~/fbsource` [unless specified otherwise],
backed by EdenFS:

```sh
$ fbclone fbsource --eden
```

Because EdenFS is already a virtual filesystem that fetches file contents
dynamically, it does not support `hg sparse`.

## Building the Code

Assuming your checkout of fbsource is at `~/fbsource`, do:

```shell
cd ~/fbsource/fbcode/eden/addons
yarn
cd ~/fbsource/fbcode/eden/addons/reviewstack
yarn codegen
```

Note that as you make changes, you may have to run the above commands again to
rebuild:

- If the dependencies in any `package.json` file changes, run `yarn` again.
- If `src/textmate/` or `src/queries/` changes, run `yarn codegen` again.

Once you have built the code, open a shell and run:

```
cd ~/fbsource/fbcode/eden/addons/reviewstack.dev
yarn start
```

and leave it running.

From there, open `http://localhost:3000/` and follow the instructions.
The development environment from this project was created using
[Create React App](https://create-react-app.dev/), so we get the benefits of hot
reloading that come with it.

Note that you will be prompted to create a [GitHub personal access token](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/creating-a-personal-access-token), which will be written to `localStorage`.
In the course of using the app, various data from GitHub may be written to
`localStorage` or `indexedDB` for the host `localhost:3000`.

**This means that if you ran another development server on port 3000 later on,
it would be able to read any of the GitHub data stored locally by
ReviewStack!** Note that if you click **Logout** in the ReviewStack UI, it will
delete all of the locally stored GitHub data, so be sure to do this before
running a different application on port 3000.

## Unit Tests

There aren't that many right now, but `yarn test` will run them.

## Linting

We could probably stand to have more ESLint rules, though we do include
`eslint:recommended`, `@typescript-eslint/recommended`, and
`typescript-eslint/eslint-recommended` in our [`.eslintrc.js`](../.eslintrc.js).

Use `yarn eslint` to run the linter.
