/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/mononoke/scs_server/metadata.h"
#include "eden/mononoke/scs/if/gen-cpp2/source_control_metadata.h" // @manual=//eden/mononoke/scs/if:source_control-cpp2

namespace facebook {
namespace scm {
namespace service {

std::unique_ptr<rust::srserver::RustThriftMetadata> create_metadata() noexcept {
  auto meta = std::make_unique<rust::srserver::RustThriftMetadata>();
  meta->meta_ =
      [](apache::thrift::metadata::ThriftServiceMetadataResponse& response) {
        ::apache::thrift::detail::md::ServiceMetadata<
            apache::thrift::ServiceHandler<
                ::facebook::scm::service::SourceControlService>>::gen(response);
      };
  return meta;
}

} // namespace service
} // namespace scm
} // namespace facebook
