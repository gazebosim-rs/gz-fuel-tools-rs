// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#include "gz/fuel_tools/Interface.hh"

#include "Bridge.hh"
namespace gz::fuel_tools {
std::string fetchResource(const std::string& uri) {
  FuelClient client;
  return fetchResourceWithClient(uri, client);
}
std::string fetchResourceWithClient(const std::string& uri, FuelClient& client) {
  Json::Value a;
  a["url"] = uri;
  a["config"] = rs::encode(client.Config());
  return rs::call("client.fetch", a).asString();
}
}  // namespace gz::fuel_tools
