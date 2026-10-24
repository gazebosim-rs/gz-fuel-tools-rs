// SPDX-License-Identifier: Apache-2.0
// Compatibility ownership/cursor adaptation. HTTP and metadata decoding run in Rust.
#include <stdexcept>

#include "Bridge.hh"
#include "ModelIterPrivate.hh"
#include "ModelPrivate.hh"
#include "WorldIterPrivate.hh"
#include "gz/fuel_tools/JSONParser.hh"
#include "gz/fuel_tools/ModelIter.hh"
#include "gz/fuel_tools/WorldIter.hh"
namespace gz::fuel_tools {
Model::Model() = default;
Model::Model(std::shared_ptr<ModelPrivate> p) : dataPtr(std::move(p)) {}
Model::operator bool() const { return bool(dataPtr); }
Model::operator bool() { return bool(dataPtr); }
ModelIdentifier Model::Identification() const { return dataPtr ? dataPtr->id : ModelIdentifier(); }
Result Model::Fetch() const {
  return Result(!dataPtr                      ? ResultType::UNKNOWN
                : dataPtr->pathOnDisk.empty() ? ResultType::FETCH_ERROR
                                              : ResultType::FETCH_ALREADY_EXISTS);
}
std::string Model::PathToModel() const { return dataPtr ? dataPtr->pathOnDisk : std::string(); }
ModelIterPrivate::~ModelIterPrivate() = default;
IterIds::~IterIds() = default;
IterIds::IterIds(std::vector<ModelIdentifier> input) : ids(std::move(input)), idIter(ids.begin()) {
  if (idIter != ids.end()) {
    auto p = std::make_shared<ModelPrivate>();
    p->id = *idIter;
    model = Model(p);
  }
}
void IterIds::Next() {
  if (idIter != ids.end()) ++idIter;
  if (idIter != ids.end()) {
    auto p = std::make_shared<ModelPrivate>();
    p->id = *idIter;
    model = Model(p);
  }
}
bool IterIds::HasReachedEnd() { return idIter == ids.end(); }
IterModels::~IterModels() = default;
IterModels::IterModels(std::vector<Model> input)
    : models(std::move(input)), modelIter(models.begin()) {
  if (modelIter != models.end()) model = *modelIter;
}
void IterModels::Next() {
  if (modelIter != models.end()) ++modelIter;
  if (modelIter != models.end()) model = *modelIter;
}
bool IterModels::HasReachedEnd() { return modelIter == models.end(); }
IterRestIds::~IterRestIds() = default;
IterRestIds::IterRestIds(const Rest& r, const ServerConfig& s, const std::string& p)
    : config(s), rest(r), api(p), idIter(ids.end()) {
  Next();
}
void IterRestIds::Next() {
  if (idIter != ids.end()) ++idIter;
  if (idIter == ids.end()) {
    if (currentPage && ids.empty()) return;
    Json::Value a;
    a["server"] = rs::encode(config);
    a["path"] = api;
    a["kind"] = "models";
    a["page"] = Json::UInt64(++currentPage);
    a["user_agent"] = rest.UserAgent();
    const auto values = rs::call("command.page", a);
    ids.clear();
    for (const auto& v : values) {
      ModelIdentifier id;
      rs::decode(v, id);
      ids.push_back(id);
    }
    idIter = ids.begin();
  }
  if (idIter != ids.end()) {
    auto p = std::make_shared<ModelPrivate>();
    p->id = *idIter;
    model = Model(p);
  }
}
bool IterRestIds::HasReachedEnd() { return idIter == ids.end(); }
ModelIter::ModelIter(std::unique_ptr<ModelIterPrivate> p) : dataPtr(std::move(p)) {}
ModelIter::ModelIter(ModelIter&& other) = default;
ModelIter::~ModelIter() = default;
ModelIter::operator bool() { return dataPtr && !dataPtr->HasReachedEnd(); }
ModelIter::operator bool() const { return dataPtr && !dataPtr->HasReachedEnd(); }
ModelIter& ModelIter::operator++() {
  if (dataPtr && !dataPtr->HasReachedEnd()) dataPtr->Next();
  return *this;
}
Model& ModelIter::operator*() {
  if (!*this) throw std::out_of_range("Fuel model iterator exhausted");
  return dataPtr->model;
}
Model* ModelIter::operator->() { return &operator*(); }
ModelIter ModelIterFactory::Create(const std::vector<ModelIdentifier>& v) {
  return ModelIter(std::make_unique<IterIds>(v));
}
ModelIter ModelIterFactory::Create(const std::vector<Model>& v) {
  return ModelIter(std::make_unique<IterModels>(v));
}
ModelIter ModelIterFactory::Create(const Rest& r, const ServerConfig& s, const std::string& p) {
  return ModelIter(std::make_unique<IterRestIds>(r, s, p));
}
ModelIter ModelIterFactory::Create() { return Create(std::vector<ModelIdentifier>()); }
WorldIterPrivate::~WorldIterPrivate() = default;
WorldIterIds::~WorldIterIds() = default;
WorldIterIds::WorldIterIds(std::vector<WorldIdentifier> input)
    : ids(std::move(input)), idIter(ids.begin()) {
  if (idIter != ids.end()) worldId = *idIter;
}
void WorldIterIds::Next() {
  if (idIter != ids.end()) ++idIter;
  if (idIter != ids.end()) worldId = *idIter;
}
bool WorldIterIds::HasReachedEnd() { return idIter == ids.end(); }
WorldIterRestIds::~WorldIterRestIds() = default;
WorldIterRestIds::WorldIterRestIds(const Rest& r, const ServerConfig& s, const std::string& p)
    : config(s), rest(r), idIter(ids.end()), path(p) {
  Next();
}
void WorldIterRestIds::Next() {
  if (idIter != ids.end()) ++idIter;
  if (idIter == ids.end()) {
    if (page && ids.empty()) return;
    Json::Value a;
    a["server"] = rs::encode(config);
    a["path"] = path;
    a["kind"] = "worlds";
    a["page"] = Json::UInt64(++page);
    a["user_agent"] = rest.UserAgent();
    const auto values = rs::call("command.page", a);
    ids.clear();
    for (const auto& v : values) {
      WorldIdentifier id;
      rs::decode(v, id);
      ids.push_back(id);
    }
    idIter = ids.begin();
  }
  if (idIter != ids.end()) worldId = *idIter;
}
bool WorldIterRestIds::HasReachedEnd() { return idIter == ids.end(); }
WorldIter::WorldIter(std::unique_ptr<WorldIterPrivate> p) : dataPtr(std::move(p)) {}
WorldIter::WorldIter(WorldIter&& other) = default;
WorldIter::~WorldIter() = default;
WorldIter::operator bool() const { return dataPtr && !dataPtr->HasReachedEnd(); }
WorldIter::operator WorldIdentifier() const {
  return dataPtr ? dataPtr->worldId : WorldIdentifier();
}
WorldIter& WorldIter::operator++() {
  if (dataPtr && !dataPtr->HasReachedEnd()) dataPtr->Next();
  return *this;
}
WorldIdentifier& WorldIter::operator*() {
  if (!*this) throw std::out_of_range("Fuel world iterator exhausted");
  return dataPtr->worldId;
}
WorldIdentifier* WorldIter::operator->() { return &operator*(); }
WorldIter WorldIterFactory::Create(const std::vector<WorldIdentifier>& v) {
  return WorldIter(std::make_unique<WorldIterIds>(v));
}
WorldIter WorldIterFactory::Create(const Rest& r, const ServerConfig& s, const std::string& p) {
  return WorldIter(std::make_unique<WorldIterRestIds>(r, s, p));
}
WorldIter WorldIterFactory::Create() { return Create(std::vector<WorldIdentifier>()); }
}  // namespace gz::fuel_tools
