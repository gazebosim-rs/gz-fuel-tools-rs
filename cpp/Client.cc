// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#include <stdexcept>

#include "Bridge.hh"
#include "LocalCache.hh"
#include "ModelIterPrivate.hh"
#include "WorldIterPrivate.hh"
#include "gz/fuel_tools/FuelClient.hh"
#include "gz/fuel_tools/Result.hh"
namespace gz::fuel_tools {
class FuelClientPrivate {
 public:
  ClientConfig config;

 public:
  Rest rest;

 public:
  Json::Value args(const std::string& kind = "models") const {
    Json::Value a;
    a["config"] = rs::encode(config);
    a["kind"] = kind;
    return a;
  }
};
FuelClient::FuelClient() : FuelClient(ClientConfig(), Rest()) {}
FuelClient::FuelClient(const ClientConfig& c, const Rest& r)
    : dataPtr(std::make_unique<FuelClientPrivate>()) {
  dataPtr->config = c;
  dataPtr->rest = r;
  dataPtr->rest.SetUserAgent(c.UserAgent());
}
FuelClient::~FuelClient() = default;
ClientConfig& FuelClient::Config() { return dataPtr->config; }
static Result response(const Json::Value& v) {
  return Result(static_cast<ResultType>(v["result"].asInt()));
}
static bool parse(const FuelClientPrivate& p, const common::URI& u, const std::string& kind,
                  bool file, Json::Value& out) {
  if (!u.Valid()) return false;
  auto a = p.args(kind);
  a["url"] = u.Str();
  a["file"] = file;
  out = rs::call("client.parse", a);
  return !out.isNull();
}
bool FuelClient::ParseModelUrl(const common::URI& u, ModelIdentifier& id) {
  Json::Value o;
  if (!parse(*dataPtr, u, "models", false, o)) return false;
  rs::decode(o["id"], id);
  return true;
}
bool FuelClient::ParseModelFileUrl(const common::URI& u, ModelIdentifier& id, std::string& file) {
  Json::Value o;
  if (!parse(*dataPtr, u, "models", true, o)) return false;
  rs::decode(o["id"], id);
  file = o["file"].asString();
  return true;
}
Result FuelClient::ModelDetails(const ModelIdentifier& id, ModelIdentifier& out) const {
  return ModelDetails(id, out, {});
}
Result FuelClient::ModelDetails(const ModelIdentifier& id, ModelIdentifier& out,
                                const std::vector<std::string>& h) const {
  auto a = dataPtr->args("models");
  a["id"] = rs::encode(id);
  a["headers"] = rs::strings(h);
  auto v = rs::call("client.details", a);
  if (v.isMember("id")) rs::decode(v["id"], out);
  return response(v);
}
Result FuelClient::DownloadModel(const common::URI& u, std::string& path) {
  ModelIdentifier id;
  if (!ParseModelUrl(u, id)) return Result(ResultType::FETCH_ERROR);
  auto a = dataPtr->args("models");
  a["id"] = rs::encode(id);
  auto v = rs::call("client.download", a);
  if (v.isMember("path")) path = v["path"].asString();
  return response(v);
}
Result FuelClient::CachedModel(const common::URI& u, std::string& path) {
  ModelIdentifier id;
  if (!ParseModelUrl(u, id)) return Result(ResultType::FETCH_ERROR);
  auto a = dataPtr->args("models");
  a["id"] = rs::encode(id);
  a["exact"] = true;
  auto v = rs::call("io.cache_list", a);
  if (v.empty()) return Result(ResultType::FETCH_ERROR);
  path = v[0]["path"].asString();
  return Result(ResultType::FETCH_ALREADY_EXISTS);
}
bool FuelClient::CachedModel(const common::URI& u) {
  std::string p;
  return bool(CachedModel(u, p));
}
Result FuelClient::CachedModelFile(const common::URI& u, std::string& path) {
  auto a = dataPtr->args("models");
  a["url"] = u.Str();
  auto v = rs::call("client.cached_file", a);
  if (v.isMember("path")) path = v["path"].asString();
  return response(v);
}
bool FuelClient::UpdateModels(const std::vector<std::string>& h) {
  auto a = dataPtr->args("models");
  a["headers"] = rs::strings(h);
  return rs::call("client.update", a).asBool();
}
bool FuelClient::ParseWorldUrl(const common::URI& u, WorldIdentifier& id) {
  Json::Value o;
  if (!parse(*dataPtr, u, "worlds", false, o)) return false;
  rs::decode(o["id"], id);
  return true;
}
bool FuelClient::ParseWorldFileUrl(const common::URI& u, WorldIdentifier& id, std::string& file) {
  Json::Value o;
  if (!parse(*dataPtr, u, "worlds", true, o)) return false;
  rs::decode(o["id"], id);
  file = o["file"].asString();
  return true;
}
Result FuelClient::WorldDetails(const WorldIdentifier& id, WorldIdentifier& out) const {
  return WorldDetails(id, out, {});
}
Result FuelClient::WorldDetails(const WorldIdentifier& id, WorldIdentifier& out,
                                const std::vector<std::string>& h) const {
  auto a = dataPtr->args("worlds");
  a["id"] = rs::encode(id);
  a["headers"] = rs::strings(h);
  auto v = rs::call("client.details", a);
  if (v.isMember("id")) rs::decode(v["id"], out);
  return response(v);
}
Result FuelClient::DownloadWorld(const common::URI& u, std::string& path) {
  WorldIdentifier id;
  if (!ParseWorldUrl(u, id)) return Result(ResultType::FETCH_ERROR);
  auto a = dataPtr->args("worlds");
  a["id"] = rs::encode(id);
  auto v = rs::call("client.download", a);
  if (v.isMember("path")) path = v["path"].asString();
  return response(v);
}
Result FuelClient::CachedWorld(const common::URI& u, std::string& path) {
  WorldIdentifier id;
  if (!ParseWorldUrl(u, id)) return Result(ResultType::FETCH_ERROR);
  auto a = dataPtr->args("worlds");
  a["id"] = rs::encode(id);
  a["exact"] = true;
  auto v = rs::call("io.cache_list", a);
  if (v.empty()) return Result(ResultType::FETCH_ERROR);
  path = v[0]["path"].asString();
  return Result(ResultType::FETCH_ALREADY_EXISTS);
}
bool FuelClient::CachedWorld(const common::URI& u) {
  std::string p;
  return bool(CachedWorld(u, p));
}
Result FuelClient::CachedWorldFile(const common::URI& u, std::string& path) {
  auto a = dataPtr->args("worlds");
  a["url"] = u.Str();
  auto v = rs::call("client.cached_file", a);
  if (v.isMember("path")) path = v["path"].asString();
  return response(v);
}
bool FuelClient::UpdateWorlds(const std::vector<std::string>& h) {
  auto a = dataPtr->args("worlds");
  a["headers"] = rs::strings(h);
  return rs::call("client.update", a).asBool();
}
bool FuelClient::ParseCollectionUrl(const common::URI& u, CollectionIdentifier& id) {
  Json::Value o;
  if (!parse(*dataPtr, u, "collections", false, o)) return false;
  rs::decode(o["id"], id);
  return true;
}
Result FuelClient::DownloadModel(const ModelIdentifier& id) { return DownloadModel(id, {}); }
Result FuelClient::DownloadModel(const ModelIdentifier& id, const std::vector<std::string>& h) {
  auto a = dataPtr->args();
  a["id"] = rs::encode(id);
  a["headers"] = rs::strings(h);
  return response(rs::call("client.download", a));
}
Result FuelClient::DownloadModel(const ModelIdentifier& id, const std::vector<std::string>& h,
                                 std::vector<ModelIdentifier>& deps) {
  auto a = dataPtr->args();
  a["id"] = rs::encode(id);
  a["headers"] = rs::strings(h);
  a["recursive"] = false;
  auto v = rs::call("client.download", a);
  deps.clear();
  for (const auto& d : v["dependencies"]) {
    ModelIdentifier i;
    rs::decode(d, i);
    deps.push_back(i);
  }
  return response(v);
}
Result FuelClient::DownloadWorld(WorldIdentifier& id) { return DownloadWorld(id, {}); }
Result FuelClient::DownloadWorld(WorldIdentifier& id, const std::vector<std::string>& h) {
  auto a = dataPtr->args("worlds");
  a["id"] = rs::encode(id);
  a["headers"] = rs::strings(h);
  auto v = rs::call("client.download", a);
  if (v.isMember("id")) rs::decode(v["id"], id);
  return response(v);
}
Result FuelClient::CachedModel(const ModelIdentifier& id, std::string& path) {
  auto a = dataPtr->args();
  a["id"] = rs::encode(id);
  a["exact"] = true;
  auto v = rs::call("io.cache_list", a);
  if (v.empty()) return Result(ResultType::FETCH_ERROR);
  path = v[0]["path"].asString();
  return Result(ResultType::FETCH_ALREADY_EXISTS);
}
Result FuelClient::ModelDependencies(const ModelIdentifier& id,
                                     std::vector<ModelIdentifier>& deps) {
  auto a = dataPtr->args();
  a["id"] = rs::encode(id);
  auto v = rs::call("client.dependencies", a);
  deps.clear();
  for (const auto& d : v["dependencies"]) {
    ModelIdentifier i;
    rs::decode(d, i);
    deps.push_back(i);
  }
  return response(v);
}
Result FuelClient::ModelDependencies(const std::vector<ModelIdentifier>& ids,
                                     std::vector<ModelIdentifier>& deps) {
  auto a = dataPtr->args();
  a["ids"] = Json::Value(Json::arrayValue);
  for (const auto& id : ids) a["ids"].append(rs::encode(id));
  a["recursive"] = true;
  auto v = rs::call("client.dependencies", a);
  deps.clear();
  for (const auto& d : v["dependencies"]) {
    ModelIdentifier i;
    rs::decode(d, i);
    deps.push_back(i);
  }
  return response(v);
}
Result FuelClient::UploadModel(const std::string& path, const ModelIdentifier& id,
                               const std::vector<std::string>& h, bool priv) {
  return UploadModel(path, id, h, priv, "");
}
Result FuelClient::UploadModel(const std::string& path, const ModelIdentifier& id,
                               const std::vector<std::string>& h, bool priv,
                               const std::string& owner) {
  auto a = dataPtr->args();
  a["id"] = rs::encode(id);
  a["path"] = path;
  a["headers"] = rs::strings(h);
  a["private"] = priv;
  a["owner"] = owner;
  return response(rs::call("client.upload", a));
}
Result FuelClient::PatchModel(const ModelIdentifier& id, const std::vector<std::string>& h) {
  return PatchModel(id, h, "");
}
Result FuelClient::PatchModel(const ModelIdentifier& id, const std::vector<std::string>& h,
                              const std::string& path) {
  auto a = dataPtr->args();
  a["id"] = rs::encode(id);
  a["path"] = path;
  a["headers"] = rs::strings(h);
  a["private"] = id.Private();
  a["owner"] = id.Owner();
  return response(rs::call("client.patch", a));
}
Result FuelClient::DeleteUrl(const common::URI& u, const std::vector<std::string>& h) {
  Json::Value id;
  std::string kind;
  if (parse(*dataPtr, u, "models", false, id))
    kind = "models";
  else if (parse(*dataPtr, u, "worlds", false, id))
    kind = "worlds";
  else
    return Result(ResultType::DELETE_ERROR);
  auto a = dataPtr->args(kind);
  a["id"] = id["id"];
  a["headers"] = rs::strings(h);
  return response(rs::call("client.delete", a));
}
void FuelClient::PopulateLicenses(const ServerConfig& server) {
  auto a = dataPtr->args();
  a["id"]["server"] = rs::encode(server);
  rs::call("client.licenses", a);
}
void FuelClient::AddServerConfigParametersToHeaders(const ServerConfig& server,
                                                    std::vector<std::string>& h) const {
  auto a = dataPtr->args();
  a["id"]["server"] = rs::encode(server);
  a["headers"] = rs::strings(h);
  auto v = rs::call("client.headers", a);
  h.clear();
  for (const auto& x : v) h.push_back(x.asString());
}
ModelIter FuelClient::Models(const ServerConfig& s) {
  return static_cast<const FuelClient*>(this)->Models(s);
}
ModelIter FuelClient::Models(const ServerConfig& s) const {
  // The historical server-wide overload falls back to local resources when
  // the first remote page cannot be obtained. Later iterator failures remain
  // explicit, so a partial remote listing cannot silently become a cache list.
  try {
    auto it = ModelIterFactory::Create(dataPtr->rest, s, "models");
    if (it) return it;
  } catch (const std::runtime_error&) {
  }
  ModelIdentifier id;
  id.SetServer(s);
  LocalCache c(&dataPtr->config);
  return c.MatchingModels(id);
}
ModelIter FuelClient::Models(const ModelIdentifier& id) { return Models(id, true); }
ModelIter FuelClient::Models(const ModelIdentifier& id) const { return Models(id, true); }
ModelIter FuelClient::Models(const ModelIdentifier& id, bool cache) {
  return static_cast<const FuelClient*>(this)->Models(id, cache);
}
ModelIter FuelClient::Models(const ModelIdentifier& id, bool cache) const {
  if (cache) {
    LocalCache c(&dataPtr->config);
    auto it = c.MatchingModels(id);
    if (it) return it;
  }
  auto a = dataPtr->args();
  a["id"] = rs::encode(id);
  auto path = rs::call("client.list_path", a).asString();
  if (path.empty()) return ModelIterFactory::Create();
  return ModelIterFactory::Create(dataPtr->rest, id.Server(), path);
}
ModelIter FuelClient::Models(const CollectionIdentifier& id) const {
  auto a = dataPtr->args();
  a["id"] = rs::encode(id);
  a["collection"] = true;
  return ModelIterFactory::Create(dataPtr->rest, id.Server(),
                                  rs::call("client.list_path", a).asString());
}
WorldIter FuelClient::Worlds(const ServerConfig& s) const {
  // The historical server-wide overload falls back to local resources when
  // the first remote page cannot be obtained. Later iterator failures remain
  // explicit, so a partial remote listing cannot silently become a cache list.
  try {
    auto it = WorldIterFactory::Create(dataPtr->rest, s, "worlds");
    if (it) return it;
  } catch (const std::runtime_error&) {
  }
  WorldIdentifier id;
  id.SetServer(s);
  LocalCache c(&dataPtr->config);
  return c.MatchingWorlds(id);
}
WorldIter FuelClient::Worlds(const WorldIdentifier& id) const {
  LocalCache c(&dataPtr->config);
  auto it = c.MatchingWorlds(id);
  if (it) return it;
  auto a = dataPtr->args("worlds");
  a["id"] = rs::encode(id);
  return WorldIterFactory::Create(dataPtr->rest, id.Server(),
                                  rs::call("client.list_path", a).asString());
}
WorldIter FuelClient::Worlds(const CollectionIdentifier& id) const {
  auto a = dataPtr->args("worlds");
  a["id"] = rs::encode(id);
  a["collection"] = true;
  return WorldIterFactory::Create(dataPtr->rest, id.Server(),
                                  rs::call("client.list_path", a).asString());
}
std::vector<FuelClient::ModelResult> FuelClient::DownloadModels(
    const std::vector<ModelIdentifier>& ids, size_t jobs) {
  auto a = dataPtr->args();
  a["ids"] = Json::Value(Json::arrayValue);
  for (const auto& id : ids) a["ids"].append(rs::encode(id));
  a["jobs"] = Json::UInt64(jobs);
  auto v = rs::call("client.batch", a);
  std::vector<ModelResult> out;
  for (const auto& r : v) {
    ModelIdentifier id;
    rs::decode(r.isMember("requested_id") ? r["requested_id"] : r["id"], id);
    out.emplace_back(id, response(r));
  }
  return out;
}
Result FuelClient::DownloadWorlds(const std::vector<WorldIdentifier>& ids, size_t jobs) {
  auto a = dataPtr->args("worlds");
  a["ids"] = Json::Value(Json::arrayValue);
  for (const auto& id : ids) a["ids"].append(rs::encode(id));
  a["jobs"] = Json::UInt64(jobs);
  auto v = rs::call("client.batch", a);
  return Result(v["ok"].asBool() ? ResultType::FETCH : ResultType::FETCH_ERROR);
}
}  // namespace gz::fuel_tools
