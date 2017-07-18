#ifndef PORTABILITY_INET_H
#define PORTABILITY_INET_H

#if defined(_MSC_VER)
	#include <winsock2.h>
	#pragma comment(lib, "Ws2_32.lib")
	/* See https://fburl.com/7hd350j8 for more details about Ws2_32.lib */
#else
	#include <arpa/inet.h>
#endif

#endif /* PORTABILITY_INET_H */

