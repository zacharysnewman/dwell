#include <doctest/doctest.h>

#include <cstring>

#include "dwell_net.h"

TEST_CASE("dwell-net C ABI is linked and reports its version") {
  CHECK(dwell_net_abi_version() == DWELL_NET_ABI_VERSION);
  const char* version = dwell_net_crate_version();
  REQUIRE(version != nullptr);
  CHECK(std::strlen(version) > 0);
}
