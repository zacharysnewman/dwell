#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>

#include "dwell/core/block_types.h"
#include "dwell/core/blocks.gen.h"

// The block registry (docs/BLOCK_REGISTRY.md, ARCHITECTURE.md §6.1): namespaced blocks with typed
// properties, canonical state strings and dense runtime state ids. The tables are generated from
// shared/blocks/*.json; this is the lookup and string layer over them.
namespace dwell::core {

const BlockDef& BlockOf(MaterialId state);

// The canonical string of a state (`dwell:stone`, `dwell:ladder[facing=north,flooded=false]`):
// every property present, keys alphabetical. Unknown ids read as air.
std::string_view StateString(MaterialId state);

// Parses `ns:name` or `ns:name[k=v,…]`: any key order, missing properties take their defaults
// (the first declared value). nullopt (and `error`, if given) for an unknown block, property or
// value, a repeated key, or malformed text.
std::optional<MaterialId> ParseState(std::string_view text, std::string* error = nullptr);

// The block with a namespaced id, or null.
const BlockDef* FindBlock(std::string_view id);

// The value of a state's property, if the block has it.
std::optional<std::string_view> StateProperty(MaterialId state, std::string_view property);

// The same block with `property` set to `value` (rotation, flooding); nullopt if the block has no
// such property or value.
std::optional<MaterialId> WithProperty(MaterialId state, std::string_view property,
                                       std::string_view value);

// FNV-1a 64 over the canonical strings (each followed by '\n') in id order, computed from the
// tables. Equals kRegistryHash; the Welcome message carries it (§8.3).
std::uint64_t ComputeRegistryHash();

}  // namespace dwell::core
