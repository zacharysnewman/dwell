#include "dwell/core/block_registry.h"

#include <vector>

#include "dwell/core/voxel.h"

namespace dwell::core {
namespace {

// The value index of each of a block's properties in a state (mixed radix, last varies fastest).
std::vector<std::size_t> Indices(const BlockDef& block, MaterialId state) {
  std::vector<std::size_t> out(block.property_count);
  std::size_t rest = state - block.first_state;
  for (std::size_t k = block.property_count; k-- > 0;) {
    const std::size_t n = kProperties[block.property_begin + k].value_count;
    out[k] = rest % n;
    rest /= n;
  }
  return out;
}

MaterialId StateOf(const BlockDef& block, const std::vector<std::size_t>& indices) {
  std::size_t id = 0;
  for (std::size_t k = 0; k < block.property_count; ++k) {
    id = id * kProperties[block.property_begin + k].value_count + indices[k];
  }
  return static_cast<MaterialId>(block.first_state + id);
}

std::optional<MaterialId> Fail(std::string* error, std::string message) {
  if (error) *error = std::move(message);
  return std::nullopt;
}

}  // namespace

const BlockDef& BlockOf(MaterialId state) { return kBlocks[GetMaterial(state).block]; }

std::string_view StateString(MaterialId state) { return GetMaterial(state).name; }

const BlockDef* FindBlock(std::string_view id) {
  for (const BlockDef& b : kBlocks) {
    if (b.id == id) return &b;
  }
  return nullptr;
}

std::optional<MaterialId> ParseState(std::string_view text, std::string* error) {
  std::string_view id = text, list;
  bool has_list = false;
  if (const auto open = text.find('['); open != std::string_view::npos) {
    if (text.back() != ']') return Fail(error, "missing ']' in \"" + std::string(text) + "\"");
    id = text.substr(0, open);
    list = text.substr(open + 1, text.size() - open - 2);
    has_list = true;
  }
  const BlockDef* block = FindBlock(id);
  if (!block) return Fail(error, "unknown block \"" + std::string(id) + "\"");
  std::vector<std::size_t> indices(block->property_count, 0);  // defaults: the first value
  std::vector<bool> given(block->property_count, false);
  while (has_list && !list.empty()) {
    const auto comma = list.find(',');
    const std::string_view pair = list.substr(0, comma);
    list = comma == std::string_view::npos ? std::string_view{} : list.substr(comma + 1);
    if (comma != std::string_view::npos && list.empty()) {
      return Fail(error, "trailing ',' in \"" + std::string(text) + "\"");
    }
    const auto eq = pair.find('=');
    if (eq == std::string_view::npos)
      return Fail(error, "expected key=value: \"" + std::string(pair) + "\"");
    const std::string_view key = pair.substr(0, eq), value = pair.substr(eq + 1);
    std::size_t k = 0;
    while (k < block->property_count && kProperties[block->property_begin + k].name != key) ++k;
    if (k == block->property_count) {
      return Fail(error, std::string(id) + " has no property \"" + std::string(key) + "\"");
    }
    if (given[k]) return Fail(error, "repeated property \"" + std::string(key) + "\"");
    given[k] = true;
    const PropertyDef& p = kProperties[block->property_begin + k];
    std::size_t v = 0;
    while (v < p.value_count && kPropertyValues[p.value_begin + v] != value) ++v;
    if (v == p.value_count) {
      return Fail(error, std::string(id) + "." + std::string(key) + " has no value \"" +
                             std::string(value) + "\"");
    }
    indices[k] = v;
  }
  return StateOf(*block, indices);
}

std::optional<std::string_view> StateProperty(MaterialId state, std::string_view property) {
  if (state >= Materials::kCount) return std::nullopt;
  const BlockDef& block = BlockOf(state);
  const auto indices = Indices(block, state);
  for (std::size_t k = 0; k < block.property_count; ++k) {
    const PropertyDef& p = kProperties[block.property_begin + k];
    if (p.name == property) return kPropertyValues[p.value_begin + indices[k]];
  }
  return std::nullopt;
}

std::optional<MaterialId> WithProperty(MaterialId state, std::string_view property,
                                       std::string_view value) {
  if (state >= Materials::kCount) return std::nullopt;
  const BlockDef& block = BlockOf(state);
  auto indices = Indices(block, state);
  for (std::size_t k = 0; k < block.property_count; ++k) {
    const PropertyDef& p = kProperties[block.property_begin + k];
    if (p.name != property) continue;
    for (std::size_t v = 0; v < p.value_count; ++v) {
      if (kPropertyValues[p.value_begin + v] == value) {
        indices[k] = v;
        return StateOf(block, indices);
      }
    }
    return std::nullopt;
  }
  return std::nullopt;
}

std::uint64_t ComputeRegistryHash() {
  std::uint64_t h = 0xcbf29ce484222325ull;
  const auto mix = [&h](unsigned char byte) { h = (h ^ byte) * 0x100000001b3ull; };
  for (const MaterialInfo& m : kMaterials) {
    for (const char c : m.name) mix(static_cast<unsigned char>(c));
    mix('\n');
  }
  return h;
}

}  // namespace dwell::core
