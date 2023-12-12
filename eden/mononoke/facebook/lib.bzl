load(
    "@fbsource//tools/build_defs:audit_dependencies_test.bzl",
    "audit_dependencies_test",
)

def tokio_dependency_test(name, rule):
    audit_dependencies_test(
        name = name,
        blocklist_patterns = [
            # we don't want to depend on tokio 0.2
            "fbsource//third-party/rust:tokio-02",
            # tokio-executor is tokio 0.1. We want to get rid of it.
            "fbsource//third-party/rust:tokio-executor",
        ],
        contacts = ["oncall+scm_server_infra@xmail.facebook.com"],
        # use exclude_subtrees_of_rules to not traverse the dep tree of
        # specific deps. Of course, having to exclude some depds creates a
        # blind spot if those deps change, but sometimes that's the only
        # solution. Leave a comment explaining why this is OK.
        exclude_subtrees_of_rules = [
            # This is fine because that crate depends on Tokio 1.0 only for a
            # trait impl on a foreign type. It's not used at runtime.
            "//common/rust/srserver:srserver",
            # This provides a 0.1 version.
            "//common/rust/shed/stats:stats",
            # This has modules for tokio 0.1, 0.2 and 1.x  It doesn't actually
            # rely on runtime Tokio though, just Io traits.
            "fbsource//third-party/rust:async-compression",
            # This does depend on Tokio 0.2, but only for the client. We don't
            # actually use the client though, only the SQL query formatting.
            "fbsource//third-party/rust:mysql_async",
        ],
        rule = rule,
    )
