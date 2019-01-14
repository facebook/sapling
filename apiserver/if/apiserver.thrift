include "common/fb303/if/fb303.thrift"

namespace py scm.mononoke.apiserver.thrift.apiserver
namespace py3 scm.mononoke.apiserver.thrift

enum MononokeAPIExceptionKind {
  InvalidInput = 1,
  NotFound = 2,
  InternalError = 3,
}

exception MononokeAPIException {
  1: MononokeAPIExceptionKind kind,
  2: string reason,
}

struct MononokeGetRawParams {
  1: string repo,
  2: string changeset,
  3: binary path,
}

service MononokeAPIService extends fb303.FacebookService {
  binary get_raw(1: MononokeGetRawParams params)
    throws (1: MononokeAPIException e),
}
