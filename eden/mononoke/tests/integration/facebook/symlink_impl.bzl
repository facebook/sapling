def _symlink_impl(ctx):
    if type(ctx.attrs.srcs) == type({}):
        srcs = ctx.attrs.srcs
    else:
        srcs = {src.short_path: src for src in ctx.attrs.srcs}

    output = ctx.actions.symlinked_dir(ctx.label.name, srcs)
    return [DefaultInfo(default_outputs = [output])]

symlink_v2 = rule(
    impl = _symlink_impl,
    attrs = {
        "srcs": attrs.named_set(attrs.source(allow_directory = True), sorted = False, default = []),
    },
)
