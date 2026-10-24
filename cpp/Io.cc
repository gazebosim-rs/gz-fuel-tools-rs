// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
// Apache-2.0. C++ API adaptation; transport and archive algorithms live in Rust.
#include "Bridge.hh"
#include "gz/fuel_tools/Interface.hh"
#include "gz/fuel_tools/RestClient.hh"
#include "gz/fuel_tools/Zip.hh"

namespace gz::fuel_tools {
bool Zip::Compress(const std::string& src, const std::string& dst) {
  Json::Value a;
  a["src"] = src;
  a["dst"] = dst;
  return rs::call("io.compress", a).asBool();
}
bool Zip::Extract(const std::string& src, const std::string& dst) {
  Json::Value a;
  a["src"] = src;
  a["dst"] = dst;
  return rs::call("io.extract", a).asBool();
}
RestResponse Rest::Request(HttpMethod method, const std::string& url, const std::string& version,
                           const std::string& path, const std::vector<std::string>& query,
                           const std::vector<std::string>& headers, const std::string& data,
                           const std::multimap<std::string, std::string>& form) const {
  Json::Value a;
  a["method"] = static_cast<unsigned>(method);
  a["url"] = url;
  a["version"] = version;
  a["path"] = path;
  a["user_agent"] = userAgent;
  a["query"] = Json::Value(Json::arrayValue);
  a["headers"] = Json::Value(Json::arrayValue);
  a["form"] = Json::Value(Json::arrayValue);
  for (const auto& v : query) a["query"].append(v);
  for (const auto& v : headers) a["headers"].append(v);
  for (const auto& [key, value] : form) {
    Json::Value pair(Json::arrayValue);
    pair.append(key);
    pair.append(value);
    a["form"].append(pair);
  }
  RestResponse response;
  const auto output = rs::call_binary("io.request", a, data, &response.data);
  response.statusCode = output["status"].asInt();
  for (const auto& key : output["headers"].getMemberNames())
    response.headers[key] = output["headers"][key].asString();
  return response;
}
void Rest::SetUserAgent(const std::string& agent) { userAgent = agent; }
const std::string& Rest::UserAgent() const { return userAgent; }
std::string sdfFromPath(const std::string& path) {
  Json::Value a;
  a["path"] = path;
  return rs::call("io.sdf", a).asString();
}
}  // namespace gz::fuel_tools
