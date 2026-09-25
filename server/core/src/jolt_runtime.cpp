#include "dwell/core/jolt_runtime.h"

#include <Jolt/Jolt.h>

#include <Jolt/Core/Factory.h>
#include <Jolt/RegisterTypes.h>

#define DWELL_STR2(x) #x
#define DWELL_STR(x) DWELL_STR2(x)

namespace dwell::core {

JoltRuntime::JoltRuntime() {
  JPH::RegisterDefaultAllocator();
  JPH::Factory::sInstance = new JPH::Factory();
  JPH::RegisterTypes();
}

JoltRuntime::~JoltRuntime() {
  JPH::UnregisterTypes();
  delete JPH::Factory::sInstance;
  JPH::Factory::sInstance = nullptr;
}

const char* JoltVersionString() {
  return "Jolt " DWELL_STR(JPH_VERSION_MAJOR) "." DWELL_STR(JPH_VERSION_MINOR) "." DWELL_STR(
      JPH_VERSION_PATCH);
}

}  // namespace dwell::core
