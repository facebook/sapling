def expose_binary(env, path, rule):
    # see lib.sh for an explanation of this format
    key = "__".join(["MONONOKE_BOOTSTRAP", path.replace("/", "SLASH").replace(".", "DOT"), rule])
    if path == ".":
        path = ""
    else:
        path = "/{}".format(path)
    target = "//eden/mononoke{}:{}".format(path, rule)
    value = "$(exe_target {})".format(target)
    env[key] = value

def env_for_binaries(binaries):
    env = {"USE_ENV_BINARIES": "1"}
    for (path, rule) in binaries:
        expose_binary(env, path, rule)
    return env

def binaries_cmd(binaries):
    env = {}
    for (path, rule) in binaries:
        expose_binary(env, path, rule)
    return "echo %s > ${OUT}" % " ".join(env.values())
