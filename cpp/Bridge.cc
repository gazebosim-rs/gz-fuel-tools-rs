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
FuelBuffer gz_fuel_rs_call(const uint8_t*, size_t);
void gz_fuel_rs_free(FuelBuffer);
}
namespace gz::fuel_tools::rs {
Json::Value call(const std::string& op, const Json::Value& args) {
  Json::Value request;
  request["op"] = op;
  request["args"] = args;
  Json::StreamWriterBuilder writer;
  writer["indentation"] = "";
  const auto input = Json::writeString(writer, request);
  const auto buffer = gz_fuel_rs_call(reinterpret_cast<const uint8_t*>(input.data()), input.size());
  struct Release {
    FuelBuffer buffer;
    ~Release() { gz_fuel_rs_free(buffer); }
  } release{buffer};
  if (!buffer.data) throw std::runtime_error("Fuel Rust bridge allocation failed");
  Json::CharReaderBuilder builder;
  std::unique_ptr<Json::CharReader> reader(builder.newCharReader());
  Json::Value output;
  std::string error;
  auto begin = reinterpret_cast<const char*>(buffer.data);
  if (!reader->parse(begin, begin + buffer.len, &output, &error))
    throw std::runtime_error("Invalid Fuel Rust response: " + error);
  if (output.isMember("error")) throw std::runtime_error(output["error"].asString());
  return output["value"];
}
}  // namespace gz::fuel_tools::rs
