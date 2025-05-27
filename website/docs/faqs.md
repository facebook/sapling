# FAQs

## How do I use Sapling while maintaining compatibility with "steamlocomotive"?
Sapling provides a way for you to configure its behaviour when it's run without any subcommands inside or outside a repository.

To make `sl` resolve to the steam locomotive UNIX command (the original `sl`), you can create an alias and configure the no-repo behaviour to call it, by running:
```
sl config --user 'alias.steamlocomotive=!/full/path/to/steamlocomotive' 'commands.naked-default.no-repo=steamlocomotive'
```

If you also want to run the steamlocomotive when inside a repo, you can also add `'commands.naked-default.in-repo=steamlocomotive'` to the end of the command.
