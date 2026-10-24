// Apache-2.0. Historical private cache interface backed by the Rust cache.
#include "Bridge.hh"
#include "LocalCache.hh"
#include "ModelIterPrivate.hh"
#include "ModelPrivate.hh"
#include "WorldIterPrivate.hh"
#include "gz/fuel_tools/ClientConfig.hh"

namespace gz::fuel_tools {
class LocalCachePrivate {
 public:
  const ClientConfig* config = nullptr;
};
namespace {
Json::Value arguments(const ClientConfig* config, const char* kind) {
  Json::Value a;
  a["config"] = config ? rs::encode(*config) : Json::Value();
  a["kind"] = kind;
  return a;
}
Json::Value saveArguments(const ClientConfig* config, const char* kind, const Json::Value& id,
                          const std::string& data, bool overwrite) {
  auto a = arguments(config, kind);
  a["id"] = id;
  a["overwrite"] = overwrite;
  a["data"] = Json::Value(Json::arrayValue);
  for (unsigned char value : data) a["data"].append(static_cast<unsigned>(value));
  return a;
}
}  // namespace
LocalCache::LocalCache(const ClientConfig* config)
    : dataPtr(std::make_shared<LocalCachePrivate>()) {
  dataPtr->config = config;
}
LocalCache::~LocalCache() = default;
ModelIter LocalCache::AllModels() {
  std::vector<Model> models;
  for (const auto& v : rs::call("io.cache_list", arguments(dataPtr->config, "models"))) {
    auto ptr = std::make_shared<ModelPrivate>();
    rs::decode(v["id"], ptr->id);
    ptr->pathOnDisk = v["path"].asString();
    models.push_back(Model(ptr));
  }
  return ModelIterFactory::Create(models);
}
WorldIter LocalCache::AllWorlds() const {
  std::vector<WorldIdentifier> ids;
  for (const auto& v : rs::call("io.cache_list", arguments(dataPtr->config, "worlds"))) {
    WorldIdentifier id;
    rs::decode(v["id"], id);
    id.SetLocalPath(v["path"].asString());
    ids.push_back(id);
  }
  return WorldIterFactory::Create(ids);
}
Model LocalCache::MatchingModel(const ModelIdentifier& id) {
  auto a = arguments(dataPtr->config, "models");
  a["id"] = rs::encode(id);
  a["exact"] = true;
  const auto values = rs::call("io.cache_list", a);
  if (values.empty()) return Model();
  auto ptr = std::make_shared<ModelPrivate>();
  rs::decode(values[0]["id"], ptr->id);
  ptr->pathOnDisk = values[0]["path"].asString();
  return Model(ptr);
}
bool LocalCache::MatchingWorld(WorldIdentifier& id) const {
  auto a = arguments(dataPtr->config, "worlds");
  a["id"] = rs::encode(id);
  a["exact"] = true;
  const auto values = rs::call("io.cache_list", a);
  if (values.empty()) return false;
  rs::decode(values[0]["id"], id);
  id.SetLocalPath(values[0]["path"].asString());
  return true;
}
ModelIter LocalCache::MatchingModels(const ModelIdentifier& id) {
  auto a = arguments(dataPtr->config, "models");
  a["id"] = rs::encode(id);
  std::vector<Model> models;
  for (const auto& v : rs::call("io.cache_list", a)) {
    auto ptr = std::make_shared<ModelPrivate>();
    rs::decode(v["id"], ptr->id);
    ptr->pathOnDisk = v["path"].asString();
    models.push_back(Model(ptr));
  }
  return ModelIterFactory::Create(models);
}
WorldIter LocalCache::MatchingWorlds(const WorldIdentifier& id) const {
  auto a = arguments(dataPtr->config, "worlds");
  a["id"] = rs::encode(id);
  std::vector<WorldIdentifier> ids;
  for (const auto& v : rs::call("io.cache_list", a)) {
    WorldIdentifier value;
    rs::decode(v["id"], value);
    value.SetLocalPath(v["path"].asString());
    ids.push_back(value);
  }
  return WorldIterFactory::Create(ids);
}
bool LocalCache::SaveModel(const ModelIdentifier& id, const std::string& data, bool overwrite) {
  if (!dataPtr->config) return false;
  return rs::call("io.cache_save",
                  saveArguments(dataPtr->config, "models", rs::encode(id), data, overwrite))["ok"]
      .asBool();
}
bool LocalCache::SaveWorld(WorldIdentifier& id, const std::string& data, bool overwrite) {
  if (!dataPtr->config) return false;
  const auto output = rs::call(
      "io.cache_save", saveArguments(dataPtr->config, "worlds", rs::encode(id), data, overwrite));
  if (!output["ok"].asBool()) return false;
  rs::decode(output["id"], id);
  id.SetLocalPath(output["path"].asString());
  return true;
}
}  // namespace gz::fuel_tools
