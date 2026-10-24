// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
// C++ source-compatible value and reference storage. Algorithms live in Rust.
#include <gz/common/Filesystem.hh>

#include "Bridge.hh"
#include "gz/fuel_tools/Helpers.hh"
#include "gz/fuel_tools/Result.hh"
namespace gz::fuel_tools {
namespace {
Json::Value args(const char* key, const Json::Value& v) {
  Json::Value a;
  a[key] = v;
  return a;
}
std::string format(const char* op, const char* key, const Json::Value& v, const std::string& p,
                   bool pretty = false) {
  auto a = args(key, v);
  a["prefix"] = p;
  a["pretty"] = pretty;
  return rs::call(op, a).asString();
}
}  // namespace
class ServerConfigPrivate {
 public:
  Json::Value v = rs::call("server_default");
  common::URI url{v["url"].asString(), true};
};
ServerConfig::ServerConfig() : dataPtr(std::make_unique<ServerConfigPrivate>()) {}
ServerConfig::ServerConfig(const ServerConfig& v)
    : dataPtr(std::make_unique<ServerConfigPrivate>(*v.dataPtr)) {}
ServerConfig& ServerConfig::operator=(const ServerConfig& v) {
  *dataPtr = *v.dataPtr;
  return *this;
}
ServerConfig::~ServerConfig() = default;
void ServerConfig::Clear() {
  dataPtr->v = rs::call("server_clear");
  dataPtr->url.Clear();
}
common::URI ServerConfig::Url() const { return dataPtr->url; }
void ServerConfig::SetUrl(const common::URI& v) {
  dataPtr->url = v;
  dataPtr->v["url"] = v.Str();
}
std::string ServerConfig::ApiKey() const { return dataPtr->v["api_key"].asString(); }
void ServerConfig::SetApiKey(const std::string& v) { dataPtr->v["api_key"] = v; }
std::string ServerConfig::Version() const { return dataPtr->v["version"].asString(); }
void ServerConfig::SetVersion(const std::string& v) { dataPtr->v["version"] = v; }
std::string ServerConfig::AsString(const std::string& p) const {
  return format("server_format", "server", rs::encode(*this), p);
}
std::string ServerConfig::AsPrettyString(const std::string& p) const {
  return format("server_format", "server", rs::encode(*this), p, true);
}
class ClientConfigPrivate {
 public:
  Json::Value v;
  std::vector<ServerConfig> servers;
  std::string userAgent;
  void update(const Json::Value& x) {
    v = x;
    servers.clear();
    for (const auto& s : x["servers"]) {
      ServerConfig c;
      rs::decode(s, c);
      servers.push_back(c);
    }
    userAgent = x["user_agent"].asString();
  }
};
ClientConfig::ClientConfig() : dataPtr(std::make_unique<ClientConfigPrivate>()) {
  dataPtr->update(rs::call("config_default"));
}
ClientConfig::ClientConfig(const ClientConfig& v)
    : dataPtr(std::make_unique<ClientConfigPrivate>(*v.dataPtr)) {}
ClientConfig& ClientConfig::operator=(const ClientConfig& v) {
  *dataPtr = *v.dataPtr;
  return *this;
}
ClientConfig::~ClientConfig() = default;
void ClientConfig::Clear() { dataPtr->update(rs::call("config_clear")); }
void ClientConfig::SetUserAgent(const std::string& v) { dataPtr->userAgent = v; }
const std::string& ClientConfig::UserAgent() const { return dataPtr->userAgent; }
bool ClientConfig::LoadConfig(const std::string& path) {
  auto a = args("config", rs::encode(*this));
  a["path"] = path;
  auto r = rs::call("config_load", a);
  dataPtr->update(r["config"]);
  return r["ok"].asBool();
}
std::string ClientConfig::ConfigPath() const { return dataPtr->v["config_path"].asString(); }
std::vector<ServerConfig> ClientConfig::Servers() const { return dataPtr->servers; }
std::vector<ServerConfig>& ClientConfig::MutableServers() const { return dataPtr->servers; }
void ClientConfig::AddServer(const ServerConfig& s) { dataPtr->servers.push_back(s); }
std::string ClientConfig::CacheLocation() const { return dataPtr->v["cache_location"].asString(); }
void ClientConfig::SetCacheLocation(const std::string& v) { dataPtr->v["cache_location"] = v; }
std::string ClientConfig::AsString(const std::string& p) const {
  return format("config_format", "config", rs::encode(*this), p);
}
class ModelIdentifierPrivate {
 public:
  Json::Value v = rs::call("id_default");
  ServerConfig server;
};
ModelIdentifier::ModelIdentifier() : dataPtr(std::make_unique<ModelIdentifierPrivate>()) {}
ModelIdentifier::ModelIdentifier(const ModelIdentifier& v)
    : dataPtr(std::make_unique<ModelIdentifierPrivate>(*v.dataPtr)) {}
ModelIdentifier& ModelIdentifier::operator=(const ModelIdentifier& v) {
  *dataPtr = *v.dataPtr;
  return *this;
}
ModelIdentifier::~ModelIdentifier() = default;
bool ModelIdentifier::operator==(const ModelIdentifier& v) const {
  return UniqueName() == v.UniqueName();
}
ServerConfig& ModelIdentifier::Server() const { return dataPtr->server; }
bool ModelIdentifier::SetServer(const ServerConfig& v) {
  if (!v.Url().Valid()) return false;
  dataPtr->server = v;
  return true;
}
std::string ModelIdentifier::UniqueName() const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  return rs::call("id_unique", a).asString();
}
common::URI ModelIdentifier::Url() const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  return common::URI(rs::call("id_url", a).asString(), true);
}
std::string ModelIdentifier::AsString(const std::string& p) const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["prefix"] = p;
  a["pretty"] = false;
  return rs::call("id_format", a).asString();
}
std::string ModelIdentifier::AsPrettyString(const std::string& p) const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["prefix"] = p;
  a["pretty"] = true;
  return rs::call("id_format", a).asString();
}
std::string ModelIdentifier::Name() const { return dataPtr->v["name"].asString(); }
bool ModelIdentifier::SetName(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "name";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string ModelIdentifier::Owner() const { return dataPtr->v["owner"].asString(); }
bool ModelIdentifier::SetOwner(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "owner";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string ModelIdentifier::Description() const { return dataPtr->v["description"].asString(); }
bool ModelIdentifier::SetDescription(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "description";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
unsigned int ModelIdentifier::FileSize() const { return dataPtr->v["file_size"].asUInt(); }
bool ModelIdentifier::SetFileSize(unsigned int v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "file_size";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::time_t ModelIdentifier::UploadDate() const {
  return static_cast<std::time_t>(dataPtr->v["upload_date"].asInt64());
}
bool ModelIdentifier::SetUploadDate(const std::time_t& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "upload_date";
  a["value"] = Json::Int64(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::time_t ModelIdentifier::ModifyDate() const {
  return static_cast<std::time_t>(dataPtr->v["modify_date"].asInt64());
}
bool ModelIdentifier::SetModifyDate(const std::time_t& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "modify_date";
  a["value"] = Json::Int64(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
uint32_t ModelIdentifier::LikeCount() const { return dataPtr->v["like_count"].asUInt(); }
bool ModelIdentifier::SetLikeCount(uint32_t v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "like_count";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
uint32_t ModelIdentifier::DownloadCount() const { return dataPtr->v["download_count"].asUInt(); }
bool ModelIdentifier::SetDownloadCount(uint32_t v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "download_count";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string ModelIdentifier::LicenseName() const { return dataPtr->v["license_name"].asString(); }
bool ModelIdentifier::SetLicenseName(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "license_name";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string ModelIdentifier::LicenseUrl() const { return dataPtr->v["license_url"].asString(); }
bool ModelIdentifier::SetLicenseUrl(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "license_url";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string ModelIdentifier::LicenseImageUrl() const {
  return dataPtr->v["license_image_url"].asString();
}
bool ModelIdentifier::SetLicenseImageUrl(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "license_image_url";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::vector<std::string> ModelIdentifier::Tags() const {
  return [&]() {
    std::vector<std::string> x;
    for (const auto& t : dataPtr->v["tags"]) x.push_back(t.asString());
    return x;
  }();
}
bool ModelIdentifier::SetTags(const std::vector<std::string>& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "tags";
  a["value"] = rs::strings(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
unsigned int ModelIdentifier::Version() const { return dataPtr->v["version"].asUInt(); }
bool ModelIdentifier::SetVersion(unsigned int v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "version";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string ModelIdentifier::VersionStr() const {
  return rs::call("version", args("id", rs::encode(*this))).asString();
}
bool ModelIdentifier::SetVersionStr(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "models";
  a["field"] = "version_str";
  a["value"] = v;
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
bool ModelIdentifier::Private() const { return dataPtr->v["private"].asBool(); }
void ModelIdentifier::SetPrivate(bool v) const { dataPtr->v["private"] = v; }
bool ModelIdentifier::operator!=(const ModelIdentifier& v) const { return !(*this == v); }
class WorldIdentifierPrivate {
 public:
  Json::Value v = rs::call("id_default");
  ServerConfig server;
};
WorldIdentifier::WorldIdentifier() : dataPtr(std::make_unique<WorldIdentifierPrivate>()) {}
WorldIdentifier::WorldIdentifier(const WorldIdentifier& v)
    : dataPtr(std::make_unique<WorldIdentifierPrivate>(*v.dataPtr)) {}
WorldIdentifier& WorldIdentifier::operator=(const WorldIdentifier& v) {
  *dataPtr = *v.dataPtr;
  return *this;
}
WorldIdentifier::~WorldIdentifier() = default;
bool WorldIdentifier::operator==(const WorldIdentifier& v) const {
  return UniqueName() == v.UniqueName();
}
ServerConfig& WorldIdentifier::Server() const { return dataPtr->server; }
bool WorldIdentifier::SetServer(const ServerConfig& v) {
  if (!v.Url().Valid()) return false;
  dataPtr->server = v;
  return true;
}
std::string WorldIdentifier::UniqueName() const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "worlds";
  return rs::call("id_unique", a).asString();
}
common::URI WorldIdentifier::Url() const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "worlds";
  return common::URI(rs::call("id_url", a).asString(), true);
}
std::string WorldIdentifier::AsString(const std::string& p) const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "worlds";
  a["prefix"] = p;
  a["pretty"] = false;
  return rs::call("id_format", a).asString();
}
std::string WorldIdentifier::AsPrettyString(const std::string& p) const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "worlds";
  a["prefix"] = p;
  a["pretty"] = true;
  return rs::call("id_format", a).asString();
}
std::string WorldIdentifier::Name() const { return dataPtr->v["name"].asString(); }
bool WorldIdentifier::SetName(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "worlds";
  a["field"] = "name";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string WorldIdentifier::Owner() const { return dataPtr->v["owner"].asString(); }
bool WorldIdentifier::SetOwner(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "worlds";
  a["field"] = "owner";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
unsigned int WorldIdentifier::Version() const { return dataPtr->v["version"].asUInt(); }
bool WorldIdentifier::SetVersion(unsigned int v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "worlds";
  a["field"] = "version";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string WorldIdentifier::LocalPath() const { return dataPtr->v["local_path"].asString(); }
bool WorldIdentifier::SetLocalPath(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "worlds";
  a["field"] = "local_path";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string WorldIdentifier::VersionStr() const {
  return rs::call("version", args("id", rs::encode(*this))).asString();
}
bool WorldIdentifier::SetVersionStr(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "worlds";
  a["field"] = "version_str";
  a["value"] = v;
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
bool WorldIdentifier::Private() const { return dataPtr->v["private"].asBool(); }
void WorldIdentifier::SetPrivate(bool v) { dataPtr->v["private"] = v; }
class CollectionIdentifierPrivate {
 public:
  Json::Value v = rs::call("id_default");
  ServerConfig server;
};
CollectionIdentifier::CollectionIdentifier()
    : dataPtr(std::make_unique<CollectionIdentifierPrivate>()) {}
CollectionIdentifier::CollectionIdentifier(const CollectionIdentifier& v)
    : dataPtr(std::make_unique<CollectionIdentifierPrivate>(*v.dataPtr)) {}
CollectionIdentifier& CollectionIdentifier::operator=(const CollectionIdentifier& v) {
  *dataPtr = *v.dataPtr;
  return *this;
}
CollectionIdentifier::~CollectionIdentifier() = default;
bool CollectionIdentifier::operator==(const CollectionIdentifier& v) const {
  return UniqueName() == v.UniqueName();
}
ServerConfig& CollectionIdentifier::Server() const { return dataPtr->server; }
bool CollectionIdentifier::SetServer(const ServerConfig& v) {
  if (!v.Url().Valid()) return false;
  dataPtr->server = v;
  return true;
}
std::string CollectionIdentifier::UniqueName() const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "collections";
  return rs::call("id_unique", a).asString();
}
common::URI CollectionIdentifier::Url() const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "collections";
  return common::URI(rs::call("id_url", a).asString(), true);
}
std::string CollectionIdentifier::AsString(const std::string& p) const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "collections";
  a["prefix"] = p;
  a["pretty"] = false;
  return rs::call("id_format", a).asString();
}
std::string CollectionIdentifier::AsPrettyString(const std::string& p) const {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "collections";
  a["prefix"] = p;
  a["pretty"] = true;
  return rs::call("id_format", a).asString();
}
std::string CollectionIdentifier::Name() const { return dataPtr->v["name"].asString(); }
bool CollectionIdentifier::SetName(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "collections";
  a["field"] = "name";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
std::string CollectionIdentifier::Owner() const { return dataPtr->v["owner"].asString(); }
bool CollectionIdentifier::SetOwner(const std::string& v) {
  auto a = args("id", rs::encode(*this));
  a["kind"] = "collections";
  a["field"] = "owner";
  a["value"] = Json::Value(v);
  auto r = rs::call("id_set", a);
  if (r["ok"].asBool()) dataPtr->v = r["id"];
  return r["ok"].asBool();
}
class ResultPrivate {
 public:
  ResultType type = ResultType::UNKNOWN;
};
Result::Result() : dataPtr(std::make_unique<ResultPrivate>()) {}
Result::Result(ResultType t) : Result() { dataPtr->type = t; }
Result::Result(const Result& v) : dataPtr(std::make_unique<ResultPrivate>(*v.dataPtr)) {}
Result::Result(Result&& v) noexcept = default;
Result::~Result() = default;
Result& Result::operator=(const Result& v) {
  *dataPtr = *v.dataPtr;
  return *this;
}
Result& Result::operator=(Result&& v) noexcept = default;
ResultType Result::Type() const { return dataPtr->type; }
Result::operator bool() const {
  return rs::call("result", args("type", static_cast<unsigned>(Type())))["ok"].asBool();
}
std::string Result::ReadableResult() const {
  return rs::call("result", args("type", static_cast<unsigned>(Type())))["message"].asString();
}
std::string sanitizeAuthority(const std::string& v) {
  return rs::call("sanitize_authority", args("text", v)).asString();
}
std::string uriToPath(const common::URI& v) {
  Json::Value a;
  a["path"] = v.Path().Str();
  a["authority"] = v.Authority() ? v.Authority()->Str() : "";
  auto out = rs::call("uri_path", a).asString();
  common::changeFromUnixPath(out);
  return out;
}
namespace rs {
Json::Value encode(const ServerConfig& v) {
  Json::Value x;
  x["url"] = v.Url().Str();
  x["has_authority"] = static_cast<bool>(v.Url().Authority());
  x["version"] = v.Version();
  x["api_key"] = v.ApiKey();
  return x;
}
void decode(const Json::Value& x, ServerConfig& v) {
  v.SetUrl(common::URI(x["url"].asString(), x.get("has_authority", true).asBool()));
  v.SetVersion(x.get("version", "1.0").asString());
  v.SetApiKey(x["api_key"].asString());
}
Json::Value encode(const ClientConfig& v) {
  Json::Value x;
  x["cache_location"] = v.CacheLocation();
  x["config_path"] = v.ConfigPath();
  x["user_agent"] = v.UserAgent();
  x["servers"] = Json::Value(Json::arrayValue);
  for (const auto& srv : v.Servers()) x["servers"].append(encode(srv));
  return x;
}
void decode(const Json::Value& x, ClientConfig& v) {
  v.Clear();
  v.SetCacheLocation(x["cache_location"].asString());
  v.SetUserAgent(x["user_agent"].asString());
  for (const auto& s : x["servers"]) {
    ServerConfig srv;
    decode(s, srv);
    v.AddServer(srv);
  }
}
Json::Value encode(const ModelIdentifier& v) {
  Json::Value x;
  x["server"] = encode(v.Server());
  x["name"] = Json::Value(v.Name());
  x["owner"] = Json::Value(v.Owner());
  x["description"] = Json::Value(v.Description());
  x["file_size"] = Json::Value(v.FileSize());
  x["upload_date"] = Json::Int64(v.UploadDate());
  x["modify_date"] = Json::Int64(v.ModifyDate());
  x["like_count"] = Json::Value(v.LikeCount());
  x["download_count"] = Json::Value(v.DownloadCount());
  x["license_name"] = Json::Value(v.LicenseName());
  x["license_url"] = Json::Value(v.LicenseUrl());
  x["license_image_url"] = Json::Value(v.LicenseImageUrl());
  x["tags"] = strings(v.Tags());
  x["version"] = Json::Value(v.Version());
  x["private"] = v.Private();
  return x;
}
void decode(const Json::Value& x, ModelIdentifier& v) {
  ServerConfig srv;
  decode(x["server"], srv);
  v.Server() = srv;
  v.SetName(x["name"].asString());
  v.SetOwner(x["owner"].asString());
  v.SetDescription(x["description"].asString());
  v.SetFileSize(x["file_size"].asUInt());
  v.SetUploadDate(static_cast<std::time_t>(x["upload_date"].asInt64()));
  v.SetModifyDate(static_cast<std::time_t>(x["modify_date"].asInt64()));
  v.SetLikeCount(x["like_count"].asUInt());
  v.SetDownloadCount(x["download_count"].asUInt());
  v.SetLicenseName(x["license_name"].asString());
  v.SetLicenseUrl(x["license_url"].asString());
  v.SetLicenseImageUrl(x["license_image_url"].asString());
  v.SetTags([&]() {
    std::vector<std::string> t;
    for (const auto& s : x["tags"]) t.push_back(s.asString());
    return t;
  }());
  v.SetVersion(x["version"].asUInt());
  v.SetPrivate(x["private"].asBool());
}
Json::Value encode(const WorldIdentifier& v) {
  Json::Value x;
  x["server"] = encode(v.Server());
  x["name"] = Json::Value(v.Name());
  x["owner"] = Json::Value(v.Owner());
  x["version"] = Json::Value(v.Version());
  x["local_path"] = Json::Value(v.LocalPath());
  x["private"] = v.Private();
  return x;
}
void decode(const Json::Value& x, WorldIdentifier& v) {
  ServerConfig srv;
  decode(x["server"], srv);
  v.Server() = srv;
  v.SetName(x["name"].asString());
  v.SetOwner(x["owner"].asString());
  v.SetVersion(x["version"].asUInt());
  v.SetLocalPath(x["local_path"].asString());
  v.SetPrivate(x["private"].asBool());
}
Json::Value encode(const CollectionIdentifier& v) {
  Json::Value x;
  x["server"] = encode(v.Server());
  x["name"] = Json::Value(v.Name());
  x["owner"] = Json::Value(v.Owner());
  return x;
}
void decode(const Json::Value& x, CollectionIdentifier& v) {
  ServerConfig srv;
  decode(x["server"], srv);
  v.Server() = srv;
  v.SetName(x["name"].asString());
  v.SetOwner(x["owner"].asString());
}
}  // namespace rs
}  // namespace gz::fuel_tools
