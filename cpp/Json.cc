// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#include "Bridge.hh"
#include "gz/fuel_tools/JSONParser.hh"
namespace gz::fuel_tools {
namespace {
Json::Value parse(const std::string& text, const ServerConfig& server, const char* kind,
                  bool many) {
  Json::Value a;
  a["text"] = text;
  a["server"] = rs::encode(server);
  a["kind"] = kind;
  a["many"] = many;
  return rs::call("json_parse", a);
}
}  // namespace
ModelIdentifier JSONParser::ParseModel(const std::string& t, const ServerConfig& s) {
  ModelIdentifier id;
  rs::decode(parse(t, s, "models", false), id);
  return id;
}
WorldIdentifier JSONParser::ParseWorld(const std::string& t, const ServerConfig& s) {
  WorldIdentifier id;
  rs::decode(parse(t, s, "worlds", false), id);
  return id;
}
std::vector<ModelIdentifier> JSONParser::ParseModels(const std::string& t, const ServerConfig& s) {
  std::vector<ModelIdentifier> out;
  for (const auto& v : parse(t, s, "models", true)) {
    ModelIdentifier id;
    rs::decode(v, id);
    out.push_back(id);
  }
  return out;
}
std::vector<WorldIdentifier> JSONParser::ParseWorlds(const std::string& t, const ServerConfig& s) {
  std::vector<WorldIdentifier> out;
  for (const auto& v : parse(t, s, "worlds", true)) {
    WorldIdentifier id;
    rs::decode(v, id);
    out.push_back(id);
  }
  return out;
}
std::string JSONParser::BuildModel(ModelIter it) {
  Json::Value a;
  a["kind"] = "models";
  a["id"] = it ? rs::encode(it->Identification()) : rs::call("id_default");
  return rs::call("json_build", a).asString();
}
std::string JSONParser::BuildWorld(WorldIter it) {
  Json::Value a;
  a["kind"] = "worlds";
  a["id"] = it ? rs::encode(*it) : rs::call("id_default");
  return rs::call("json_build", a).asString();
}
bool JSONParser::ParseLicenses(const std::string& text,
                               std::map<std::string, unsigned int>& licenses) {
  Json::Value a;
  a["text"] = text;
  auto r = rs::call("json_licenses", a);
  for (const auto& k : r["licenses"].getMemberNames())
    licenses.emplace(k, r["licenses"][k].asUInt());
  return r["ok"].asBool();
}
}  // namespace gz::fuel_tools
