# This file contains macros that are shared across Eden.

def get_oss_suffix():
    '''Build rule suffix to use for open-source-specific build targets.'''
    return '-oss'

def get_daemon_versions():
    '''List of configurations to aid in creating dual build rules.

    Returns:
        An array of tuples where the first member is a build target for the
        daemon and the second member is the suffix to use for other templated
        build target names.
    '''
    return [
        ('//eden/fs/service:edenfs%s' % suffix, suffix)
        for suffix in ["", get_oss_suffix()]
    ]

def get_test_env_and_deps(suffix=''):
    '''Returns env vars and a dep list that is useful for locating various
    build products from inside our tests'''

    daemon_target = '//eden/fs/service:edenfs%s' % suffix
    env_to_target = {
        'EDENFS_CLI_PATH': '//eden/cli:eden',
        'EDENFS_SERVER_PATH': daemon_target,
        'EDENFS_POST_CLONE_PATH': '//eden/hooks/hg:post-clone',
        'EDENFS_FSATTR_BIN': '//eden/integration/helpers:fsattr',
        'EDENFS_FAKE_EDENFS': '//eden/integration/helpers:fake_edenfs',
        'EDENFS_HG_IMPORT_HELPER': '//eden/fs/store/hg:hg_import_helper',
        'EDEN_HG_BINARY': '//scm/telemetry/hg:hg',
        'HG_REAL_BIN': '//scm/hg:hg',
        'CHG_BIN': '//scm/hg:chg',
    }

    envs = {
      'EDENFS_SUFFIX': suffix,
    }
    deps = []

    for name, dep in sorted(env_to_target.items()):
        envs[name] = '$(location %s)' % dep
        deps.append(dep)

    return {
        'env': envs,
        'deps': deps
    }
