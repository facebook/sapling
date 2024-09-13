load("@fbcode_macros//build_defs:cpp_library.bzl", "cpp_library")

def sl_backing_store(cas = False):
    preffix = "cas_" if cas else ""
    cpp_library(
        name = "sapling_%sbacking_store" % preffix,
        srcs = ["SaplingBackingStore.cpp"],
        headers = ["SaplingBackingStore.h"],
        deps = [
            ":hg_proxy_hash",
            ":sapling_import_request",
            "//eden/common/telemetry:structured_logger",
            "//eden/common/utils:enum",
            "//eden/common/utils:fault_injector",
            "//eden/common/utils:path",
            "//eden/common/utils:throw",
            "//eden/common/utils:utils",
            "//eden/fs/config:config",
            "//eden/fs/service:thrift_util",
            "//eden/fs/telemetry:log_info",
            "//eden/fs/telemetry:stats",
            "//eden/fs/utils:static_assert",
            "//folly:executor",
            "//folly:string",
            "//folly/executors:cpu_thread_pool_executor",
            "//folly/executors/task_queue:unbounded_blocking_queue",
            "//folly/executors/thread_factory:init_thread_factory",
            "//folly/futures:core",
            "//folly/logging:logging",
            "//folly/portability:gflags",
            "//folly/system:thread_name",
        ],
        exported_deps = [
            "fbsource//third-party/googletest:gtest_headers",
            ":sapling_backing_store_options",
            ":sapling_import_request_queue",
            "//eden/common/telemetry:telemetry",
            "//eden/common/utils:ref_ptr",
            "//eden/fs:config",
            "//eden/fs/model:model",
            "//eden/fs/store:backing_store_interface",
            "//eden/fs/store:context",
            "//eden/fs/store:store",
            "//eden/fs/telemetry:activity_buffer",
            "//eden/scm/lib/backingstore:%sbackingstore" % preffix,  # @manual
            "//eden/scm/lib/backingstore:%sbackingstore@header" % preffix,  # @manual
            "//eden/scm/lib/backingstore:sapling_native_%sbackingstore" % preffix,
            "//folly:range",
            "//folly:synchronized",
        ],
        external_deps = [
            "re2",
        ],
    )
