// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#pragma once
#include <json/json.h>

#include <string>

#include "gz/fuel_tools/ClientConfig.hh"
#include "gz/fuel_tools/CollectionIdentifier.hh"
#include "gz/fuel_tools/ModelIdentifier.hh"
#include "gz/fuel_tools/ServerConfig.hh"
#include "gz/fuel_tools/WorldIdentifier.hh"
namespace gz::fuel_tools::rs {
Json::Value call(const std::string& op, const Json::Value& args = Json::Value(Json::objectValue));
Json::Value encode(const ServerConfig&);
Json::Value encode(const ClientConfig&);
Json::Value encode(const ModelIdentifier&);
Json::Value encode(const WorldIdentifier&);
Json::Value encode(const CollectionIdentifier&);
void decode(const Json::Value&, ServerConfig&);
void decode(const Json::Value&, ClientConfig&);
void decode(const Json::Value&, ModelIdentifier&);
void decode(const Json::Value&, WorldIdentifier&);
void decode(const Json::Value&, CollectionIdentifier&);
inline Json::Value bytes(const std::string& s) {
  Json::Value v(Json::arrayValue);
  for (unsigned char c : s) v.append(c);
  return v;
}
inline std::string unbytes(const Json::Value& v) {
  std::string s;
  s.reserve(v.size());
  for (const auto& c : v) s.push_back(static_cast<char>(c.asUInt()));
  return s;
}
inline Json::Value strings(const std::vector<std::string>& s) {
  Json::Value v(Json::arrayValue);
  for (const auto& x : s) v.append(x);
  return v;
}
}  // namespace gz::fuel_tools::rs
