// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#include "Bridge.hh"

#include <cstdint>
#include <memory>
#include <stdexcept>
extern "C" {
struct FuelBuffer {
  uint8_t* data;
  size_t len;
  size_t capacity;
};
struct FuelResponse {
  FuelBuffer metadata;
  FuelBuffer body;
};
FuelBuffer gz_fuel_rs_call(const uint8_t*, size_t);
FuelResponse gz_fuel_rs_call_binary(const uint8_t*, size_t, const uint8_t*, size_t);
void gz_fuel_rs_free(FuelBuffer);
}
namespace gz::fuel_tools::rs {
namespace {
// Rust owns these independent allocations. Release them once even if JSON
// parsing, body copying, or a caller-visible exception interrupts the adapter.
struct Release {
  FuelBuffer buffer;
  ~Release() { gz_fuel_rs_free(buffer); }
};
std::string request(const std::string& op, const Json::Value& args) {
  Json::Value value;
  value["op"] = op;
  value["args"] = args;
  Json::StreamWriterBuilder writer;
  writer["indentation"] = "";
  return Json::writeString(writer, value);
}
Json::Value decode(const FuelBuffer& buffer) {
  if (!buffer.data) throw std::runtime_error("Fuel Rust bridge allocation failed");
  Json::CharReaderBuilder builder;
  std::unique_ptr<Json::CharReader> reader(builder.newCharReader());
  Json::Value output;
  std::string error;
  const auto begin = reinterpret_cast<const char*>(buffer.data);
  if (!reader->parse(begin, begin + buffer.len, &output, &error))
    throw std::runtime_error("Invalid Fuel Rust response: " + error);
  if (output.isMember("error")) throw std::runtime_error(output["error"].asString());
  return output["value"];
}
}  // namespace
Json::Value call(const std::string& op, const Json::Value& args) {
  const auto input = request(op, args);
  const auto buffer = gz_fuel_rs_call(reinterpret_cast<const uint8_t*>(input.data()), input.size());
  const Release release{buffer};
  return decode(buffer);
}
Json::Value call_binary(const std::string& op, const Json::Value& args, const std::string& body,
                        std::string* outputBody) {
  const auto input = request(op, args);
  // Both source strings remain alive during the synchronous Rust call. Rust
  // neither changes their bytes nor retains their pointers after returning.
  const auto response =
      gz_fuel_rs_call_binary(reinterpret_cast<const uint8_t*>(input.data()), input.size(),
                             reinterpret_cast<const uint8_t*>(body.data()), body.size());
  const Release metadata{response.metadata};
  const Release output{response.body};
  auto value = decode(response.metadata);
  if (outputBody) {
    if (!response.body.data && response.body.len)
      throw std::runtime_error("Fuel Rust binary response allocation failed");
    if (response.body.len)
      outputBody->assign(reinterpret_cast<const char*>(response.body.data), response.body.len);
    else
      outputBody->clear();
  }
  return value;
}
}  // namespace gz::fuel_tools::rs
