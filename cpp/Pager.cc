// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#include "Pager.hh"

#include <json/json.h>

#include <cstdint>
#include <memory>
#include <stdexcept>
extern "C" {
struct PagerBuffer {
  uint8_t* data;
  size_t len;
  size_t capacity;
};
void* gz_fuel_rs_pager_new(const uint8_t*, size_t);
PagerBuffer gz_fuel_rs_pager_next(void*);
void gz_fuel_rs_pager_free(void*);
void gz_fuel_rs_free(PagerBuffer);
}
namespace gz::fuel_tools::rs {
Pager::Pager(const Json::Value& args) : handle(nullptr) {
  Json::StreamWriterBuilder writer;
  writer["indentation"] = "";
  const auto input = Json::writeString(writer, args);
  handle = gz_fuel_rs_pager_new(reinterpret_cast<const uint8_t*>(input.data()), input.size());
}
Pager::~Pager() { gz_fuel_rs_pager_free(handle); }
Json::Value Pager::Next() {
  const auto buffer = gz_fuel_rs_pager_next(handle);
  struct Release {
    PagerBuffer buffer;
    ~Release() { gz_fuel_rs_free(buffer); }
  } release{buffer};
  Json::CharReaderBuilder builder;
  std::unique_ptr<Json::CharReader> reader(builder.newCharReader());
  Json::Value result;
  std::string error;
  const auto begin = reinterpret_cast<const char*>(buffer.data);
  if (!reader->parse(begin, begin + buffer.len, &result, &error))
    throw std::runtime_error("Invalid Fuel pagination response");
  if (result.isMember("error")) throw std::runtime_error(result["error"].asString());
  return result["value"];
}
}  // namespace gz::fuel_tools::rs
