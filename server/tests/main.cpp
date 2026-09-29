// doctest runner. `--dwell-origin-x=<metres>` moves the player tests' local frame (test_origin.h);
// `--dwell-origin-x=far` picks kFarOriginX (~8,000 km).
#define DOCTEST_CONFIG_IMPLEMENT
#include <doctest/doctest.h>

#include <cstdlib>
#include <string_view>
#include <vector>

#include "test_origin.h"

int main(int argc, char** argv) {
  std::vector<char*> args;
  constexpr std::string_view kOrigin = "--dwell-origin-x=";
  for (int i = 0; i < argc; ++i) {
    const std::string_view arg = argv[i];
    if (arg.starts_with(kOrigin)) {
      const std::string_view value = arg.substr(kOrigin.size());
      dwell::test::OriginX() =
          value == "far" ? dwell::test::kFarOriginX : std::atoi(std::string(value).c_str());
      if (dwell::test::OriginX() % dwell::test::kOriginAlign != 0) return 2;
      continue;
    }
    args.push_back(argv[i]);
  }
  doctest::Context context(static_cast<int>(args.size()), args.data());
  return context.run();
}
