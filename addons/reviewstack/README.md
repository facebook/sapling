# ReviewStack

[ReviewStack](https://sapling-scm.com/docs/addons/reviewstack) is a novel user interface for GitHub pull requests with custom support for _stacked changes_. The user experience is inspired by Meta's internal code review tool, but leverages [GitHub's design system](https://primer.style/) to achieve a look and feel that is familiar to GitHub users:

![](./docs/reviewstack-demo.gif)

A hosted instance of ReviewStack is publicly available at https://reviewstack.dev/.
Note that it has _no server component_ (though it does leverage [Netlify's OAuth signing to authenticate with GitHub](https://docs.netlify.com/visitor-access/oauth-provider-tokens/)).

## Local Development

- Run `yarn` in the `addons/` folder to install all the dependencies.
- Run `yarn codegen` in the `addons/reviewstack` to build the generated code.
- Run `yarn start` in the `addons/reviewstack.dev` folder to run a local instance of ReviewStack.

The development environment was created using
[Create React App](https://create-react-app.dev/), so it should be available on `http://localhost:3000/` by default.

If you have already authenticated with the [GitHub CLI](https://cli.github.com/) `gh`,
you can run:

```
gh auth status -t
```

to dump your [GitHub personal access token](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/creating-a-personal-access-token) that you can use with the development instance of ReviewStack.

**WARNING:** The token will be written to `localStorage`, and in the course of using the app, various data from GitHub may be written to `localStorage` or `indexedDB` for the host `localhost:3000`. This means that if you ran another development server on port `3000` later on, **it would be able to read any of the GitHub data stored locally by ReviewStack!** Note that if you click **Logout** in the ReviewStack UI, it will delete all of the locally stored GitHub data, so be sure to do this before running a different application on port `3000`.
