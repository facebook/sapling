/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <functional>
#include "scm/service/if/gen-cpp2/source_control_metadata.h" // @manual=//scm/service/if:source_control-cpp2

namespace facebook {
namespace rust {
namespace srserver {

using MetadataFunc = std::function<void(
    ::apache::thrift::metadata::ThriftServiceMetadataResponse&)>;

struct RustThriftMetadata {
  MetadataFunc meta_;
};

} // namespace srserver
} // namespace rust

namespace scm {
namespace service {

rust::srserver::RustThriftMetadata* create_metadata() noexcept {
  using apache::thrift::can_throw;

  auto meta = new rust::srserver::RustThriftMetadata();
  meta->meta_ =
      [](apache::thrift::metadata::ThriftServiceMetadataResponse& response) {
        ::apache::thrift::detail::md::ServiceMetadata<
            ::facebook::scm::service::SourceControlServiceSvIf>::
            gen(can_throw(*response.metadata_ref()),
                can_throw(*response.context_ref()));
      };
  return meta;
}

} // namespace service
} // namespace scm

} // namespace facebook
