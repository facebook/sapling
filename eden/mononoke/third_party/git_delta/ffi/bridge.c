#ifdef CARGO_BUILD
#include "../original_sources/git-compat-util.h"
#include "../original_sources/delta.h"
#else
#include "eden/mononoke/third_party/git_delta/original_sources/git-compat-util.h"
#include "eden/mononoke/third_party/git_delta/original_sources/delta.h"
#endif

void *
git_delta_from_buffers(const void *src_buf, unsigned long src_bufsize,
	   const void *trg_buf, unsigned long trg_bufsize,
	   unsigned long *delta_size, unsigned long max_delta_size)
{
	return diff_delta(src_buf, src_bufsize, trg_buf, trg_bufsize, delta_size, max_delta_size);
}
