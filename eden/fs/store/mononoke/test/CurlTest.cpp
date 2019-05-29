/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/experimental/TestUtil.h>
#include <folly/experimental/io/FsUtil.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <proxygen/httpserver/HTTPServer.h>
#include <proxygen/httpserver/RequestHandler.h>
#include <proxygen/httpserver/ResponseBuilder.h>
#include <proxygen/httpserver/ScopedHTTPServer.h>
#include <proxygen/lib/http/HTTPMessage.h>
#include <wangle/ssl/SSLContextConfig.h>

#include "eden/fs/store/mononoke/CurlHttpClient.h"
#include "eden/fs/utils/ServiceAddress.h"

using namespace facebook::eden;
using folly::test::TemporaryDirectory;
using proxygen::ScopedHTTPServer;
using testing::HasSubstr;

namespace {
class TestServer {
 public:
  void operator()(
      const proxygen::HTTPMessage& headers,
      std::unique_ptr<folly::IOBuf> /* requestBody */,
      proxygen::ResponseBuilder& responseBuilder) {
    auto path = folly::StringPiece(headers.getPath());
    if (path.startsWith("/400")) {
      responseBuilder.status(400, "Bad request").body("hello world");
    } else {
      responseBuilder.status(200, "OK").body("hello world");
    }
  }
};

// @lint-ignore-every PRIVATEKEY1
const std::string kClientCACertName = "client-ca-cert.pem";
const std::string kClientCACertContent = folly::stripLeftMargin(R"(
  -----BEGIN CERTIFICATE-----
  MIIDmjCCAoKgAwIBAgIBATANBgkqhkiG9w0BAQsFADBQMQswCQYDVQQGEwJVUzEL
  MAkGA1UECAwCQ0ExDTALBgNVBAoMBEFzb3gxJTAjBgNVBAMMHEFzb3ggQ2VydGlm
  aWNhdGlvbiBBdXRob3JpdHkwHhcNMTcwODAzMjMyMTA1WhcNNDQxMjE5MjMyMTA1
  WjBQMQswCQYDVQQGEwJVUzELMAkGA1UECAwCQ0ExDTALBgNVBAoMBEFzb3gxJTAj
  BgNVBAMMHEFzb3ggQ2VydGlmaWNhdGlvbiBBdXRob3JpdHkwggEiMA0GCSqGSIb3
  DQEBAQUAA4IBDwAwggEKAoIBAQDfv3KonszKqaZLZ5Vwnl/v6BhwQNqyx3nEDXTY
  pCn17En3DJzsa0zlqkmw8XJeQrx6+iZjLyGqEjIcqHAFcabux7PJ5z+T41kabNzU
  +WEYBhNbEB1xRm7Rqz9OzroajWIK8Wugzmqu2Sz+QYaFPjsW85+zVB6E3YPbBpz/
  uPmAecwpInzFH7C9o5TZGoYS+0K1fH935EhM617HSVvHQflQL8IcGZuLExVxiOZ+
  SkJIXO+JaM2cXBFnqf4halHQ5O+866Xk09WbhUpOqi/tGE74VQBKC2u2F1DUga0W
  37Gwcp4o9WWdeCL10323QOSnAJoaMccBpILZSL3g7YJD1ZlFAgMBAAGjfzB9MB0G
  A1UdDgQWBBR8WSTHikglMmAbowGKfg4kFNNFbzAfBgNVHSMEGDAWgBR8WSTHikgl
  MmAbowGKfg4kFNNFbzAMBgNVHRMEBTADAQH/MC0GCCsGAQUFBwEBBCEwHzAdBggr
  BgEFBQcwAoYRaHR0cHM6Ly90ZXN0X2NlcnQwDQYJKoZIhvcNAQELBQADggEBAACe
  5R64MK058S3g6mQuviburcnKeBojMt1liqKGcCDwFKFHiYCN3hKoZmEQ4XvQu0U5
  U2a3/sFG5mZD8UjAQzlQQkdjy4CwM3iGA5EeTT+VYnc9/UQU1yeyiGIRkNDJKp2y
  p4vw5sm40uBwc+QfUAl7AExO4Q8FOdVqoS/zixYtFNQ2CjlLEzY5FRgyzfHQQDtn
  rmtdVKOeWd0itvgSCeMs5KfetZlFAHavclcAN/721ukGiaWXyxQPfRLX2dS4RB8j
  TwC15NBTsRTbYhJLYuBoUwTdhCojBUr8NN1kgwjHsT6wjtLJpRl6qeKHMw+Y9IlT
  VgbFH84VIfIB1tnMNNA=
  -----END CERTIFICATE-----
)");

const std::string kServerCertName = "server-cert.pem";
const std::string kServerCertContent = folly::stripLeftMargin(R"(
  -----BEGIN CERTIFICATE-----
  MIIDKzCCAhOgAwIBAgIBCjANBgkqhkiG9w0BAQUFADBFMQswCQYDVQQGEwJVUzEP
  MA0GA1UECgwGVGhyaWZ0MSUwIwYDVQQDDBxUaHJpZnQgQ2VydGlmaWNhdGUgQXV0
  aG9yaXR5MB4XDTE0MDUxNjIwMjg1MloXDTQxMTAwMTIwMjg1MlowRjELMAkGA1UE
  BhMCVVMxDTALBgNVBAgTBE9oaW8xETAPBgNVBAcTCEhpbGxpYXJkMRUwEwYDVQQD
  EwxBc294IENvbXBhbnkwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQCz
  ZGrJ5XQHAuMYHlBgn32OOc9l0n3RjXccio2ceeWctXkSxDP3vFyZ4kBILF1lsY1g
  o8UTjMkqSDYcytCLK0qavrv9BZRLB9FcpqJ9o4V9feaI/HsHa8DYHyEs8qyNTTNG
  YQ3i4j+AA9iDSpezIYy/tyAOAjrSquUW1jI4tzKTBh8hk8MAMvR2/NPHPkrp4gI+
  EMH6u4vWdr4F9bbriLFWoU04T9mWOMk7G+h8BS9sgINg2+v5cWvl3BC4kLk5L1yJ
  FEyuofSSCEEe6dDf7uVh+RPKa4hEkIYo31AEOPFrN56d+pCj/5l67HTWXoQx3rjy
  dNXMvgU75urm6TQe8dB5AgMBAAGjJTAjMCEGA1UdEQQaMBiHBH8AAAGHEAAAAAAA
  AAAAAAAAAAAAAAEwDQYJKoZIhvcNAQEFBQADggEBAD26XYInaEvlWZJYgtl3yQyC
  3NRQc3LG7XxWg4aFdXCxYLPRAL2HLoarKYH8GPFso57t5xnhA8WfP7iJxmgsKdCS
  0pNIicOWsMmXvYLib0j9tMCFR+a8rn3f4n+clwnqas4w/vWBJUoMgyxtkP8NNNZO
  kIl02JKRhuyiFyPLilVp5tu0e+lmyUER+ak53WjLq2yoytYAlHkzkOpc4MZ/TNt5
  UTEtx/WVlZvlrPi3dsi7QikkjQgo1wCnm7owtuAHlPDMAB8wKk4+vvIOjsGM33T/
  8ffq/4X1HeYM0w0fM+SVlX1rwkXA1RW/jn48VWFHpWbE10+m196OdiToGfm2OJI=
  -----END CERTIFICATE-----
)");

const std::string kServerKeyName = "server-key.pem";
const std::string kServerKeyContent = folly::stripLeftMargin(R"(
  -----BEGIN RSA PRIVATE KEY-----
  MIIEpAIBAAKCAQEAs2RqyeV0BwLjGB5QYJ99jjnPZdJ90Y13HIqNnHnlnLV5EsQz
  97xcmeJASCxdZbGNYKPFE4zJKkg2HMrQiytKmr67/QWUSwfRXKaifaOFfX3miPx7
  B2vA2B8hLPKsjU0zRmEN4uI/gAPYg0qXsyGMv7cgDgI60qrlFtYyOLcykwYfIZPD
  ADL0dvzTxz5K6eICPhDB+ruL1na+BfW264ixVqFNOE/ZljjJOxvofAUvbICDYNvr
  +XFr5dwQuJC5OS9ciRRMrqH0kghBHunQ3+7lYfkTymuIRJCGKN9QBDjxazeenfqQ
  o/+Zeux01l6EMd648nTVzL4FO+bq5uk0HvHQeQIDAQABAoIBAQCSPcBYindF5/Kd
  jMjVm+9M7I/IYAo1tG9vkvvSngSy9bWXuN7sjF+pCyqAK7qP1mh8acWVJGYx0+BZ
  JHVRnp8Y+3hg0hWL/PmN4EICzjVakjJHZhwddpglF2uCKurD3jV4oFIjrXE6uOfe
  UAbO/wCwoWa+RM8TQkGzljYmyiGufCcXlgEKMNA7TIvbJ9TVx3VTCOQy6EjZ13jd
  M6X7byV/ZOFpZ2H0QV46LvZraw04riXQ/59gVmzizYdI+BwnxxapsCmalTJoV/Y0
  LMI2ylat4PTMVTxPF+ti7Nt+rUkkEx6kuiAgfc+bzE4BSD5X4wy3fdLVLccoxXYw
  4N3fOuQhAoGBAOLrMhiSCrzXGjDWTbPrwzxXDO0qm+wURELi3N5SXIkKUdG2/In6
  wNdpXdvqblOm7SASgPf9KCwUSADrNw6R6nbfrrir5EHg66YydI/OW42QzJKcBUFh
  5Q5na3fvoL/zRhsmh0gEymBg+OIfNel2LY69bl8aAko2y0R1kj7zb8X1AoGBAMph
  9hlnkIBSw60+pKHaOqo2t/kihNNMFyfOgJvh8960eFeMDhMIXgxPUR8yaPX0bBMb
  bCdEJJ2pmq7zUBPvxVJLedwkGMhywElA8yYVh+S6x4Cg+lYo4spIjrHQ/WTvJkHB
  GrDskxdq80lbXjwRd0dPJZkxhKJec1o0n8S03Mn1AoGAGarK5taWGlgmYUHMVj6j
  vc6G6si4DFMaiYpJu2gLiYC+Un9lP2I6r+L+N+LjidjG16rgJazf/2Rn5Jq2hpJg
  uAODKuZekkkTvp/UaXPJDVFEooy9V3DwTNnL4SwcvbmRw35vLOlFzvMJE+K94WN5
  sbyhoGY7vhNGmL7HxREaIoUCgYEAwpteVWFz3yE2ziF9l7FMVh7V23go9zGk1n9I
  xhyJL26khbLEWeLi5L1kiTYlHdUSE3F8F2n8N6s+ddq79t/KA29WV6xSNHW7lvUg
  mk975CMC8hpZfn5ETjVlGXGYJ/Wa+QGiE9z5ODx8gt6cB/DXnLdrtRqbqrJeA7C0
  rScpY/0CgYBCC1QeuAiwWHOqQn3BwsZo9JQBTyT0QvDqLH/F+h9QbXep+4HvyAxG
  nTMNDtGyfyKGDaDUn5hyeU7Oxvzq0K9P+eZD3MjQeaMEg/++GPGUPmDUTqyb2UT8
  5s0NIUobxfKnTD6IpgOIq7ffvVY6cKBMyuLmu/gSvscsbONHjKti3Q==
  -----END RSA PRIVATE KEY-----
)");

const std::string kClientChainName = "client-chain.pem";
const std::string kClientChainContent = folly::stripLeftMargin(R"(
  -----BEGIN CERTIFICATE-----
  MIIDZjCCAk6gAwIBAgIBCjANBgkqhkiG9w0BAQsFADBQMQswCQYDVQQGEwJVUzEL
  MAkGA1UECAwCQ0ExDTALBgNVBAoMBEFzb3gxJTAjBgNVBAMMHEFzb3ggQ2VydGlm
  aWNhdGlvbiBBdXRob3JpdHkwHhcNMTcwODAzMjMyMTA2WhcNNDQxMjE5MjMyMTA2
  WjAwMQswCQYDVQQGEwJVUzENMAsGA1UECgwEQXNveDESMBAGA1UEAwwJdGVzdHVz
  ZXIxMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAxNjbq/fOD9BZN1N+
  UPwTLK0jSE3zW3o4YIIZKnOc8ln3kY6xI0Xtm96uGD/y+nWFHGiEX5eqcTZEEaX7
  4kHBU3mcWaZvC/JfBwyG1gf3/5sl5yxgJB9LsSZRcBKcET+JJ8Ps7WADBU0IVUsj
  yejt25wnpsmglXbt7rObay8H0AYstRmNhpq7N92R6CKeVpHzuvUujSFnMGmxh5ux
  l9anytFfr84E4G/AEUFkEKimFsqtG5Je3ZAfREhoNdCSERENYNtNXgLVCeWLGAT8
  Mp0gNDHgBGheiWDIFRK5PKbJENMfOZt0mjQAM3ypmsLCNhSV81CDc9Q/GEOLMMxX
  VP773wIDAQABo2swaTAJBgNVHRMEAjAAMA4GA1UdDwEB/wQEAwIFoDAdBgNVHSUE
  FjAUBggrBgEFBQgCAgYIKwYBBQUHAwIwLQYIKwYBBQUHAQEEITAfMB0GCCsGAQUF
  BzAChhFodHRwczovL3Rlc3RfY2VydDANBgkqhkiG9w0BAQsFAAOCAQEAO/kPyxvj
  ZXxb4uCjqLFiPWKcGHhX1U+rE6tHf/iBh6WJ7D751Z56AT8YjtSWXxDgD3zOvhml
  TGjkWIofG5yhYBeYEvrlHzXIqsPmUhOYgO42V4AXFB8u5gPw7tB5aozhxA4jB6Ik
  JXkmDiBsyP4DvYknl7gGl4eubOPM/MDy0YasZH3C3shmjAJEW2CEbIrLXyT+N6yK
  +zVtxEVlTZ4TPmXzOLqMIrgjLBWq5I9ATf5BCcqjs2bnHvVkErmY6WcpPTKDyCg6
  Nmtla39ffF6V1S9BItLB7nZsGwMuExwPvotFseTppiAQyZGH0rPECxyPDpNNij/G
  5ro+AZN3usfsqw==
  -----END CERTIFICATE-----
  -----BEGIN RSA PRIVATE KEY-----
  MIIEogIBAAKCAQEAxNjbq/fOD9BZN1N+UPwTLK0jSE3zW3o4YIIZKnOc8ln3kY6x
  I0Xtm96uGD/y+nWFHGiEX5eqcTZEEaX74kHBU3mcWaZvC/JfBwyG1gf3/5sl5yxg
  JB9LsSZRcBKcET+JJ8Ps7WADBU0IVUsjyejt25wnpsmglXbt7rObay8H0AYstRmN
  hpq7N92R6CKeVpHzuvUujSFnMGmxh5uxl9anytFfr84E4G/AEUFkEKimFsqtG5Je
  3ZAfREhoNdCSERENYNtNXgLVCeWLGAT8Mp0gNDHgBGheiWDIFRK5PKbJENMfOZt0
  mjQAM3ypmsLCNhSV81CDc9Q/GEOLMMxXVP773wIDAQABAoIBAGDrlV1aqa7HmuXO
  ykb9lkNNDC4xkzzbNJ7v74wjWIdLHMYiR71iVNeGEJoIAo6nBl8yZtraRiVv3pwB
  6b9BOPrsybqqY8qyD2/dDxaa3dSQg10LUFr4vb//aeGQiB9F9TYLFcDaoSIfB5dX
  Y8uqUFLs0+kfJV3yLLx22nMvuN0HD4ra8pzWOzO6r/cyGV5Q0hY+GjfbXUvO1Kk7
  ez/Hc2MSR/rQTReEldExB839lcYjlxpQ5FyfXBVsBev6DLl3YRZe0Zq3cE94BBTs
  EksD4O1maWpW9RMsLyxGiPKt08zIXdupuOQXWG+d3K/ZhVheGtQz2bqBobDANB2I
  l6o7xcECgYEA8pgXaktdxOxErDeiy0TRYg2/GVkqmzH7JK3cy9vWL67ihbCKSDzF
  H3B27PbBx/DDCRdV1zXHwY2fuqVdrNbJXCzeXg8xw6VrIwtWNKJksi5+echCOgf8
  dfpr5d/ujcVPDd/oqBFBXqTwA93H6rablLRxlIM6j74u+fvfKKYsnLkCgYEAz7mY
  1PHcpf39+SCp1gyP6tNSk5W5gkgmQYNVD1gEgYvAThJ+SYpsq/FvyutF8gQwugZk
  +ESlrMn79CxDmxmwLDkI1dhH87rK63QadcKAf6gGzmmW5iF7JpfEI2sCgHRzo+YY
  DvjXPYHORXmG8FzSwXEFeaGmkUfZkQCnlGaBAVcCgYBkeETKSuhM1CUkxe3wDVJC
  L7tDPkB9AdgnOrJE44jzOpSqFZFPlYt2F9fJD/D1Y2sC6t0sQiO2r3bFkBMZr+K5
  AAQgJF7RzkJuwxUyu0bE3KiYuy1iZ0hRfCMPkwxzPpIdBuyOHodaMSkOEN3pATOy
  BIE9ppOsUHGYKo4jgZ7cUQKBgCnUONdgoMr4O4VIM4r72psx1KYNd16pwrJcjOtb
  EQU5LA9MoAuVCU8Sfi2BdQNIHrvG/9wSjr4dqlO/+hkochZlocbWF58X1TbWmWFv
  Okr6fexgzNcolNDvrLppGQbe7E5rzhptt4IWOoA4+Zg7SOFVLgIRMCZ/LmuJkzVJ
  8t5fAoGAT5aji/j7w+isIU4R13w2x5UXyVSapmAPZt0N3KQeD5/WHMEvUgu7Lk3a
  FrBHbs1bvF6Q6uJhe+OvN0/vqhesa334XzBPoyMVa3UfTkVrW6TvEoJa/Gc8oIno
  Pjrn8wSA0SCFf/wuCcXIDhk9Mq2q5dOD5FeTPFciR7zYWFlOQnI=
  -----END RSA PRIVATE KEY-----
)");

const std::string kInvalidChainName = "invalid-chain.pem";
const std::string kInvalidChainContent = folly::stripLeftMargin(R"(
  -----BEGIN CERTIFICATE-----
  MIIC5zCCAc8CAQowDQYJKoZIhvcNAQEFBQAwQzELMAkGA1UEBhMCVVMxDTALBgNV
  BAoMBEFzb3gxJTAjBgNVBAMMHEFzb3ggQ2VydGlmaWNhdGlvbiBBdXRob3JpdHkw
  HhcNMTQwOTE5MjIxMzQ2WhcNNDIwMjA0MjIxMzQ2WjAwMQswCQYDVQQGEwJVUzEN
  MAsGA1UECgwEQXNveDESMBAGA1UEAwwJMTI3LjAuMC4xMIIBIjANBgkqhkiG9w0B
  AQEFAAOCAQ8AMIIBCgKCAQEAprvPkBYr6LTFOldRjTQ9zKf86tBu9kG1a4CLu4c1
  2twWTf04b8lfpG5qUMt13IeI5CH9ygjkLz6gZjsGDICVokG5P5fd9k+3eIv6m0K5
  rgNUXeSYJJTbfIhEw99fdI4tpu5irtnWLGGsDApF1O2NDLYh6U0+eB1OWOGhqrSU
  AMPibief2jtLsETaRZrYSFknPfgrNjzcIfhnAv3rMnkEc55knV8l7UZCLgUaRPfS
  4ZcTe1VJghHPCbbfQ6AEcHZaXhOlX0voAXesB5RVuyPMuhQzBfasBstjITIpdbQI
  AlnFuF/vo8JRhqJKjOWek6DJyH7yjw9ZtvXsMTJCun9M7wIDAQABMA0GCSqGSIb3
  DQEBBQUAA4IBAQCdghgh4hK0HUgvr+Ue2xUgAkEhQK7nvBlxw42l64zWNIkVrg3C
  sGBx1/ZV7sVrrz5P8LkoZmKcgSoaZQRhiZ9P+nBj4hUz8oFYJ2xTl2Bo1UmEoz+r
  z63WerLLb48HQLrGJN/V1Uodjb/eVRwY16qw0JoaRg3BGbO2k19jeNIfpp00atic
  xvgxZsHuRrax4PkL6ObrASILj78AOzPmKOlMk2cbS+Ol4WJNzbqFDQaR3QXv4WSR
  6td3LlJtSyMWjMnkYOOidLYsSQ5bVnWbnP/bj/apRXxX9wi7ez739Gqc4bylJgW5
  Ym+TCytFhaK6z05whWCDcD6CrXyFGer/Cqfv
  -----END CERTIFICATE-----
  -----BEGIN RSA PRIVATE KEY-----
  MIIEpAIBAAKCAQEAprvPkBYr6LTFOldRjTQ9zKf86tBu9kG1a4CLu4c12twWTf04
  b8lfpG5qUMt13IeI5CH9ygjkLz6gZjsGDICVokG5P5fd9k+3eIv6m0K5rgNUXeSY
  JJTbfIhEw99fdI4tpu5irtnWLGGsDApF1O2NDLYh6U0+eB1OWOGhqrSUAMPibief
  2jtLsETaRZrYSFknPfgrNjzcIfhnAv3rMnkEc55knV8l7UZCLgUaRPfS4ZcTe1VJ
  ghHPCbbfQ6AEcHZaXhOlX0voAXesB5RVuyPMuhQzBfasBstjITIpdbQIAlnFuF/v
  o8JRhqJKjOWek6DJyH7yjw9ZtvXsMTJCun9M7wIDAQABAoIBAQCGJrJ4Yf5uO5Q8
  vqjVDeVzVu4+F/pPlMrddg33knCYaWBg246fEs0rRdOwsiNgjoRr2ZWTCthd0uvH
  lVHmmUbLyEm+iviCB9281hOK/ILdKbyl1xk6xbJbXmDFoGHzK7o7h65KtOaHywZc
  oZ9SFNfaFGjwh7/tcNbq2I/1A1nZynTko6iLVpgV0kkQCpaweFYQMXWv/ELkFHeL
  7tFIA50XFXbNDqnAuaW6XrIrW33ZeOJfruF2OG+QVWyTBgczk0fodDZJFS5MbDXu
  UB+W1nDhakZFbugtDSXMd3BMnLZFdsa12FYTMNG050w2OTHOF5ILX+IFwzbnlboX
  fbralUKRAoGBANnjttZWcNtM4L8JkUJwXsDgTD/0d+iu5gCmSs0p9E4wGbWlu+In
  cNE6CV4Dy+GMk5GzXFR+GV/IlPvVzSOmbFsFdBi8L3c/1IrPbQv8dEosKm6BvV9O
  0zIBaPuzgU2yyBFxdpfsHynAZoLdY3rq6IJdNmJrmDcVgEfBVOExU7EVAoGBAMPl
  hqNmGi3VwHPQ2iiuM48ijPbuS0hK3dUjx2A4otOYAro86Q4egcdtyBOONhBwD89h
  I6BUo+vReV6ikI8LQfoplBaRos7qJ2e9SOxmRIJGAZPkGlFF0uljxKZ2Hdtmruae
  mJOZqKCa38sTnqWyXV/xCXE5X94EXuJP17L2Bt7zAoGAOG7gFheBV2tL8m657qlI
  AVCWryHURLG35IctbIHnQrD2l7N7PBHXCHmtn2oATkSom94GleOrEsHSxH8ViJw8
  CD8bWKS07n/bvrAGoEocnHFf9AsqTxsNXDA9TqOpY8RgSRRIEQUY9Sld45sPfvCE
  k+8sfMU9QVcSSINsRn8OHBkCgYEAgzBGD01EQOfB/42hW9b1fmjEAGYrElnY33Eb
  hyvGl29YfEJoTOVPQjAZ6ka1nCJ/5ACIrEmikT1yS1cQ+kquv4pyuv6DCpCzHP0d
  Rfti699YFSOQIFdjXJtMybGWYyUMAjO5uDcSP6QYNVaJSyv87lBsY1/p/LPumx6f
  NCEhDtMCgYBpcK4f2E+JjaGHABX5OS5Soegtgj7JjZv/M3N2OLvX2xrlkxAEPlJ5
  nvaVjikBcOsj3/+LDrBMDoEbG2JFaopiue8pW+tZWfbhJw0pf02f+hgHjCaR+1Ny
  Qqd+ERH7vFjwzc3UuZay1NbU9/wVMNsL7jWKnvsKKCk9PxG2OzP2iQ==
  -----END RSA PRIVATE KEY-----
)");
} // namespace

class CurlTest : public ::testing::Test {
 protected:
  static std::unique_ptr<ScopedHTTPServer> createServer(
      folly::fs::path&& cert,
      folly::fs::path&& key,
      folly::fs::path&& clientCA) {
    auto ssl = std::make_unique<wangle::SSLContextConfig>();
    ssl->isDefault = true;
    ssl->setCertificate(cert.native(), key.native(), "");
    ssl->clientCAFile = clientCA.native();
    return ScopedHTTPServer::start(TestServer(), 0, 4, std::move(ssl));
  }

  static std::unique_ptr<TemporaryDirectory> writeCertificates() {
    auto dir = std::make_unique<TemporaryDirectory>("certs");

    folly::writeFile(
        kClientCACertContent, (dir->path() / kClientCACertName).c_str());
    folly::writeFile(
        kServerCertContent, (dir->path() / kServerCertName).c_str());
    folly::writeFile(kServerKeyContent, (dir->path() / kServerKeyName).c_str());
    folly::writeFile(
        kClientChainContent, (dir->path() / kClientChainName).c_str());
    folly::writeFile(
        kInvalidChainContent, (dir->path() / kInvalidChainName).c_str());

    return dir;
  }

  static void SetUpTestCase() {
    certs_ = writeCertificates();
    server_ = createServer(
        certs_->path() / kServerCertName,
        certs_->path() / kServerKeyName,
        certs_->path() / kClientCACertName);

    const auto address = server_->getAddresses()[0].address;
    address_ = std::make_shared<ServiceAddress>("::1", address.getPort());
  }

  static void TearDownTestCase() {
    delete certs_.release();
    delete server_.release();
  }

  static std::unique_ptr<TemporaryDirectory> certs_;
  static std::unique_ptr<ScopedHTTPServer> server_;
  static std::shared_ptr<ServiceAddress> address_;
};

std::unique_ptr<TemporaryDirectory> CurlTest::certs_ = nullptr;
std::unique_ptr<ScopedHTTPServer> CurlTest::server_ = nullptr;
std::shared_ptr<ServiceAddress> CurlTest::address_ = nullptr;

TEST_F(CurlTest, Success) {
  auto client = CurlHttpClient(
      address_,
      AbsolutePath((certs_->path() / kClientChainName).native()),
      std::chrono::milliseconds(100000));

  auto result = client.get("/")->moveToFbString();

  EXPECT_EQ(result, "hello world");
}

TEST_F(CurlTest, InvalidClientCertificate) {
  auto client = CurlHttpClient(
      address_,
      AbsolutePath((certs_->path() / kInvalidChainName).native()),
      std::chrono::milliseconds(100000));

  try {
    client.get("/");
    EXPECT_TRUE(false); // request should throw an exception
  } catch (const std::runtime_error& ex) {
    EXPECT_THAT(ex.what(), HasSubstr("SSL connect error"));
  }
}

TEST_F(CurlTest, ThrowOn4XX) {
  auto client = CurlHttpClient(
      address_,
      AbsolutePath((certs_->path() / kClientChainName).native()),
      std::chrono::milliseconds(100000));

  try {
    client.get("/400");
    EXPECT_TRUE(false); // request should throw an exception
  } catch (const std::runtime_error& ex) {
    EXPECT_THAT(ex.what(), HasSubstr("received 400 error"));
  }
}
