# sapling-output-plugin

## Usage in Markdown

A MDX plugin that interprets `with-output` code blocks and completes the output
part of the example code.

For example:

<pre>
```with-output
$ echo a
```
</pre>

will be rendered as:

```
$ echo a
a
```

The syntax of the code block is similar to `.t` test used by sapling, without
the double space prefix. For preparation code that should be hidden from the
rendered output, use `# hide begin` and `# hide end` to mark lines as hidden:

<pre>
```with-output
# hide begin
$ sl init repo
$ cd repo
$ touch a b
# hide end
$ sl add a
$ sl st
```
</pre>

will be rendered as:

```
$ sl add a
$ sl st
A a
? b
```


## Runtime Dependency

It depends on `hg debugruntest --fix` to run a `.t` test and complete the
output portion of it.


## Development

### Recompiling for a Docusaurus project

Run `yarn install` from the Docusaurus project to apply code changes in this
plugin.

### Debugging

The plugin will generate a temporary directory prefixed `mdx-sapling-output` in
system temporary directory, and auto delete it after completion.  Set
`MDX_SAPLING_OUTPUT_DEBUG=1` to prevent auto deletion of the temporary
directory for debugging.

### TypeScript

Before building the static version of the site, be sure to run the following in
this folder:

```shell
yarn install
yarn build
```

Though if you are actively developing the plugin, run:

```shell
yarn install
yarn watch
```

and then the TypeScript watcher will update the contents of the `dist/` folder
in the background. Unfortunately, Docusaurus appears to require a restart to
pick up the change.
