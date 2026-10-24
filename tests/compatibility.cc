// SPDX-License-Identifier: Apache-2.0
// Deterministic compatibility checks supplement the unchanged upstream suites.
#include <gtest/gtest.h>

#include <algorithm>
#include <atomic>
#include <cctype>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <gz/fuel_tools.hh>
#include <mutex>
#include <stdexcept>
#include <thread>
#include <utility>
#include <vector>

#include "LocalCache.hh"
#include "ModelIterPrivate.hh"
#include "WorldIterPrivate.hh"

#ifndef _WIN32
#include <arpa/inet.h>
#include <sys/socket.h>
#include <unistd.h>
#endif

namespace {
using namespace gz::fuel_tools;
namespace fs = std::filesystem;

// A stored ZIP fixture independent of both the replacement and upstream ZIP
// implementation. Every downloaded byte, including CRC and central offsets,
// is generated here and subsequently checked through the public client API.
void le16(std::string& s, std::uint16_t n) {
  s.push_back(static_cast<char>(n));
  s.push_back(static_cast<char>(n >> 8));
}
void le32(std::string& s, std::uint32_t n) {
  le16(s, static_cast<std::uint16_t>(n));
  le16(s, static_cast<std::uint16_t>(n >> 16));
}
std::uint32_t crc32(const std::string& s) {
  std::uint32_t crc = 0xffffffff;
  for (unsigned char byte : s) {
    crc ^= byte;
    for (int i = 0; i < 8; ++i) crc = (crc >> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return crc ^ 0xffffffff;
}
std::string zip(const std::vector<std::pair<std::string, std::string>>& entries) {
  std::string local, central;
  for (const auto& [name, data] : entries) {
    const auto offset = local.size();
    const auto crc = crc32(data);
    le32(local, 0x04034b50);
    le16(local, 20);
    le16(local, 0);
    le16(local, 0);
    le16(local, 0);
    le16(local, 0);
    le32(local, crc);
    le32(local, data.size());
    le32(local, data.size());
    le16(local, name.size());
    le16(local, 0);
    local += name;
    local += data;
    le32(central, 0x02014b50);
    le16(central, 20);
    le16(central, 20);
    le16(central, 0);
    le16(central, 0);
    le16(central, 0);
    le16(central, 0);
    le32(central, crc);
    le32(central, data.size());
    le32(central, data.size());
    le16(central, name.size());
    le16(central, 0);
    le16(central, 0);
    le16(central, 0);
    le16(central, 0);
    le32(central, 0);
    le32(central, offset);
    central += name;
  }
  const auto start = local.size();
  local += central;
  le32(local, 0x06054b50);
  le16(local, 0);
  le16(local, 0);
  le16(local, entries.size());
  le16(local, entries.size());
  le32(local, central.size());
  le32(local, start);
  le16(local, 0);
  return local;
}
std::string modelZip(const std::string& dependency = "") {
  std::string metadata = "model {file: \"model.sdf\"} name: \"box\"";
  if (!dependency.empty()) metadata += " dependencies {uri: \"" + dependency + "\"}";
  return zip({{"model.config", "<model><name>box</name><sdf version='1.9'>model.sdf</sdf></model>"},
              {"metadata.pbtxt", metadata},
              {"model.sdf", "<sdf version='1.9'><model name='box'/></sdf>"},
              {"binary.dat", std::string("a\0\xff\r\n", 5)}});
}
std::string worldZip() {
  return zip({{"metadata.pbtxt", "world {file: \"arena.sdf\"} name: \"arena\""},
              {"arena.sdf", "<sdf version='1.9'><world name='arena'/></sdf>"}});
}
class TempDirectory {
 public:
  fs::path path;
  TempDirectory() {
    static std::atomic<unsigned> counter{0};
    path = fs::temp_directory_path() /
           ("fuel-cxx-compat-" +
            std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()) + "-" +
            std::to_string(counter++));
    fs::create_directories(path);
  }
  ~TempDirectory() {
    std::error_code ignored;
    fs::remove_all(path, ignored);
  }
};
void write(const fs::path& path, const std::string& data) {
  fs::create_directories(path.parent_path());
  std::ofstream out(path, std::ios::binary);
  out.write(data.data(), data.size());
}

#ifndef _WIN32
class HttpFixture {
 public:
  std::string url;
  std::mutex mutex;
  std::vector<std::string> requests;
  explicit HttpFixture(bool listening = true) {
    socket_ = ::socket(AF_INET, SOCK_STREAM, 0);
    if (socket_ < 0) throw std::runtime_error("cannot create fixture socket");
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    address.sin_port = 0;
    if (::bind(socket_, reinterpret_cast<sockaddr*>(&address), sizeof(address)) != 0 ||
        (listening && ::listen(socket_, 32) != 0)) {
      ::close(socket_);
      throw std::runtime_error("cannot bind fixture socket");
    }
    socklen_t size = sizeof(address);
    ::getsockname(socket_, reinterpret_cast<sockaddr*>(&address), &size);
    url = "http://127.0.0.1:" + std::to_string(ntohs(address.sin_port));
    if (listening) worker_ = std::thread([this] { serve(); });
  }
  ~HttpFixture() {
    stop_ = true;
    ::shutdown(socket_, SHUT_RDWR);
    ::close(socket_);
    if (worker_.joinable()) worker_.join();
  }
  std::size_t requestCount() {
    std::lock_guard<std::mutex> lock(mutex);
    return requests.size();
  }

 private:
  int socket_ = -1;
  std::atomic<bool> stop_{false};
  std::thread worker_;
  void serve() {
    while (!stop_) {
      const int client = ::accept(socket_, nullptr, nullptr);
      if (client < 0) break;
      timeval timeout{10, 0};
      ::setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
      std::string input;
      for (;;) {
        char buffer[8192];
        const auto n = ::recv(client, buffer, sizeof(buffer), 0);
        if (n <= 0) break;
        input.append(buffer, n);
        const auto end = input.find("\r\n\r\n");
        if (end == std::string::npos) continue;
        auto header = input.substr(0, end);
        std::transform(header.begin(), header.end(), header.begin(),
                       [](unsigned char c) { return std::tolower(c); });
        const auto length = header.find("content-length:");
        const auto bytes = length == std::string::npos ? 0 : std::stoul(header.substr(length + 15));
        if (input.size() >= end + 4 + bytes) break;
      }
      {
        std::lock_guard<std::mutex> lock(mutex);
        requests.push_back(input);
      }
      const auto space = input.find(' '), last = input.find(' ', space + 1);
      const auto method = input.substr(0, space);
      const auto route = input.substr(space + 1, last - space - 1);
      int status = 200;
      std::string body = "{}", type = "application/json";
      if (route.find("missing") != std::string::npos) {
        status = 404;
      } else if (route.find(".zip") != std::string::npos) {
        type = "application/zip";
        if (route.find("/worlds/") != std::string::npos)
          body = worldZip();
        else
          body = modelZip(
              route.find("/box/") != std::string::npos ? url + "/1.0/alice/models/dependency" : "");
      } else if (route.find("/licenses") != std::string::npos)
        body = "[{\"name\":\"MIT\",\"ID\":1}]";
      else if (route.find("/raw") != std::string::npos) {
        type = "application/octet-stream";
        body = std::string("a\0b\xff", 4);
      } else if (method == "GET") {
        const bool worlds = route.find("worlds") != std::string::npos;
        const std::string id = worlds ? "{\"name\":\"arena\",\"owner\":\"alice\",\"version\":2}"
                                      : "{\"name\":\"box\",\"owner\":\"alice\",\"version\":2}";
        body = route.find("page=") != std::string::npos
                   ? (route.find("page=1") != std::string::npos ? "[" + id + "]" : "[]")
                   : id;
      }
      std::string output =
          "HTTP/1.1 " + std::to_string(status) + (status == 200 ? " OK\r\n" : " Not Found\r\n");
      output += "Content-Length: " + std::to_string(body.size()) + "\r\nContent-Type: " + type;
      output += "\r\nX-Ign-Resource-Version: 2\r\nConnection: close\r\n\r\n" + body;
      std::size_t sent = 0;
      while (sent < output.size()) {
        const auto n = ::send(client, output.data() + sent, output.size() - sent, MSG_NOSIGNAL);
        if (n <= 0) break;
        sent += n;
      }
      ::close(client);
    }
  }
};
ServerConfig server(const HttpFixture& http) {
  ServerConfig s;
  s.SetUrl(gz::common::URI(http.url, true));
  s.SetVersion("1.0");
  return s;
}
ClientConfig configuration(const HttpFixture& http, const fs::path& path) {
  ClientConfig c;
  c.Clear();
  c.AddServer(server(http));
  c.SetCacheLocation(path.string());
  c.SetUserAgent("compatibility-test");
  return c;
}
ModelIdentifier model(const HttpFixture& http) {
  ModelIdentifier id;
  id.SetServer(server(http));
  id.SetOwner("Alice");
  id.SetName("Box");
  return id;
}
WorldIdentifier world(const HttpFixture& http) {
  WorldIdentifier id;
  id.SetServer(server(http));
  id.SetOwner("Alice");
  id.SetName("Arena");
  return id;
}

TEST(FacadeCompatibility, DirectTransportPreservesAllMethodsAndBinaryData) {
  HttpFixture http;
  TempDirectory temporary;
  const auto upload = temporary.path / "sample.png";
  write(upload, std::string("a\0b", 3));
  Rest rest;
  rest.SetUserAgent("fuel-cxx-test");
  const std::string& agent = rest.UserAgent();
  ASSERT_EQ("fuel-cxx-test", agent);
  for (HttpMethod method : {HttpMethod::GET, HttpMethod::POST, HttpMethod::DELETE, HttpMethod::PUT,
                            HttpMethod::PATCH, HttpMethod::POST_FORM, HttpMethod::PATCH_FORM}) {
    const auto response = rest.Request(
        method, http.url, "1.0", "raw", {"q=one", "page=1"}, {"X-Test: yes"},
        std::string("x\0y", 3),
        {{"tag", "one"}, {"tag", "two"}, {"file", "@" + upload.string() + ";texture.png"}});
    ASSERT_EQ(200, response.statusCode);
    EXPECT_EQ(std::string("a\0b\xff", 4), response.data);
    EXPECT_EQ("2", response.headers.at("X-Ign-Resource-Version"));
  }
  EXPECT_EQ(0, rest.Request(HttpMethod::GET, "", "", "", {}, {}, "").statusCode);
  ASSERT_EQ(7u, http.requestCount());
  std::lock_guard<std::mutex> lock(http.mutex);
  EXPECT_NE(std::string::npos, http.requests[1].find(std::string("x\0y", 3)));
  EXPECT_NE(std::string::npos, http.requests[5].find("filename=\"texture.png\""));
  EXPECT_NE(std::string::npos, http.requests[6].find("name=\"tag\""));
}

TEST(FacadeCompatibility, CacheSaveMatchesTipAndMutatesWorldPath) {
  HttpFixture http;
  TempDirectory temporary;
  auto config = configuration(http, temporary.path);
  LocalCache cache(&config);
  auto id = model(http);
  id.SetVersion(2);
  EXPECT_TRUE(cache.SaveModel(id, modelZip(), false));
  EXPECT_FALSE(cache.SaveModel(id, modelZip(), false));
  EXPECT_TRUE(cache.SaveModel(id, modelZip(), true));
  const auto cached = cache.MatchingModel(id);
  ASSERT_TRUE(cached);
  EXPECT_EQ(ResultType::FETCH_ALREADY_EXISTS, cached.Fetch().Type());
  EXPECT_TRUE(fs::is_regular_file(fs::path(cached.PathToModel()) / "binary.dat"));
  FuelClient client(config);
  std::string path;
  EXPECT_TRUE(client.CachedModel(id, path));
  EXPECT_EQ(cached.PathToModel(), path);
  EXPECT_TRUE(client.CachedModel(gz::common::URI(http.url + "/1.0/alice/models/box", true)));
  std::string file;
  ModelIdentifier parsedModel;
  const gz::common::URI modelFile(http.url + "/1.0/alice/models/box/2/files/binary.dat", true);
  EXPECT_TRUE(client.ParseModelFileUrl(modelFile, parsedModel, file));
  EXPECT_EQ("binary.dat", file);
  EXPECT_TRUE(client.CachedModelFile(modelFile, path));
  EXPECT_EQ((fs::path(cached.PathToModel()) / "binary.dat").string(), path);
  EXPECT_EQ((fs::path(cached.PathToModel()) / "model.sdf").string(),
            sdfFromPath(cached.PathToModel()));
  auto wid = world(http);
  wid.SetVersion(2);
  EXPECT_TRUE(cache.SaveWorld(wid, worldZip(), false));
  EXPECT_FALSE(wid.LocalPath().empty());
  EXPECT_FALSE(cache.SaveWorld(wid, worldZip(), false));
  EXPECT_TRUE(cache.SaveWorld(wid, worldZip(), true));
  EXPECT_TRUE(client.CachedWorld(gz::common::URI(http.url + "/1.0/alice/worlds/arena", true)));
  WorldIdentifier parsedWorld;
  const gz::common::URI worldFile(http.url + "/1.0/alice/worlds/arena/2/files/arena.sdf", true);
  EXPECT_TRUE(client.ParseWorldFileUrl(worldFile, parsedWorld, file));
  EXPECT_EQ("arena.sdf", file);
  EXPECT_TRUE(client.CachedWorldFile(worldFile, path));
  EXPECT_EQ((fs::path(wid.LocalPath()) / "arena.sdf").string(), path);
  EXPECT_FALSE(client.CachedModelFile(
      gz::common::URI(http.url + "/1.0/alice/models/box/2/files/missing", true), path));
  auto worlds = cache.AllWorlds();
  ASSERT_TRUE(worlds);
  EXPECT_EQ("arena", worlds->Name());
  LocalCache absent(nullptr);
  EXPECT_FALSE(absent.SaveModel(id, modelZip(), false));
  EXPECT_FALSE(absent.SaveWorld(wid, worldZip(), false));
  EXPECT_FALSE(absent.AllModels());
  EXPECT_FALSE(absent.AllWorlds());
  EXPECT_FALSE(absent.MatchingModel(id));
  EXPECT_FALSE(absent.MatchingWorld(wid));
  auto incomplete = ModelIdentifier();
  EXPECT_FALSE(cache.SaveModel(incomplete, modelZip(), false));
  auto incompleteWorld = WorldIdentifier();
  EXPECT_FALSE(cache.SaveWorld(incompleteWorld, worldZip(), false));
  const Model empty;
  EXPECT_FALSE(empty);
  EXPECT_EQ(ResultType::UNKNOWN, empty.Fetch().Type());
  EXPECT_TRUE(empty.PathToModel().empty());
}

TEST(FacadeCompatibility, AllClientOverloadsForwardResultsAndMutableArguments) {
  HttpFixture http;
  TempDirectory temporary;
  auto config = configuration(http, temporary.path / "cache");
  FuelClient client(config);
  const FuelClient& constant = client;
  client.Config().SetUserAgent("changed-agent");
  EXPECT_EQ("changed-agent", client.Config().UserAgent());
  auto id = model(http);
  auto wid = world(http);
  ModelIdentifier detail;
  WorldIdentifier worldDetail;
  ASSERT_TRUE(constant.ModelDetails(id, detail));
  EXPECT_EQ("box", detail.Name());
  ASSERT_TRUE(constant.ModelDetails(id, detail, {"X-Test: yes"}));
  EXPECT_EQ(2u, detail.Version());
  ASSERT_TRUE(constant.WorldDetails(wid, worldDetail));
  EXPECT_EQ("arena", worldDetail.Name());
  ASSERT_TRUE(constant.WorldDetails(wid, worldDetail, {"X-Test: yes"}));
  EXPECT_EQ(2u, worldDetail.Version());
  std::vector<ModelIdentifier> dependencies;
  ASSERT_TRUE(client.DownloadModel(id, {}, dependencies));
  ASSERT_EQ(1u, dependencies.size());
  EXPECT_EQ("dependency", dependencies.front().Name());
  ASSERT_TRUE(client.ModelDependencies(id, dependencies));
  ASSERT_EQ(1u, dependencies.size());
  ASSERT_TRUE(client.ModelDependencies(std::vector<ModelIdentifier>{id}, dependencies));
  ASSERT_EQ(1u, dependencies.size());
  ASSERT_TRUE(client.DownloadModel(id));
  ASSERT_TRUE(client.DownloadModel(id, {"X-Test: yes"}));
  ASSERT_TRUE(client.DownloadWorld(wid));
  EXPECT_EQ(2u, wid.Version());
  EXPECT_FALSE(wid.LocalPath().empty());
  ASSERT_TRUE(client.DownloadWorld(wid, {"X-Test: yes"}));
  std::string path;
  ASSERT_TRUE(
      client.DownloadWorld(gz::common::URI(http.url + "/1.0/alice/worlds/arena", true), path));
  EXPECT_TRUE(fs::is_regular_file(fs::path(path) / "arena.sdf"));
  ASSERT_TRUE(
      client.DownloadModel(gz::common::URI(http.url + "/1.0/alice/models/box", true), path));
  EXPECT_TRUE(fs::is_regular_file(fs::path(path) / "model.sdf"));
  auto batch = client.DownloadModels({id}, 1);
  ASSERT_GE(batch.size(), 1u);
  EXPECT_TRUE(std::get<1>(batch.front()));
  EXPECT_EQ("box", std::get<0>(batch.front()).Name());
  EXPECT_TRUE(client.DownloadWorlds({wid}, 1));
  auto missing = id;
  missing.SetName("missing");
  EXPECT_FALSE(client.DownloadModel(missing));
  auto badWorld = wid;
  badWorld.SetName("missing");
  EXPECT_FALSE(client.DownloadWorlds({badWorld}, 1));
  EXPECT_TRUE(client.UpdateModels({}));
  EXPECT_TRUE(client.UpdateWorlds({}));
  client.PopulateLicenses(server(http));
  const auto upload = temporary.path / "upload";
  write(upload / "model.config",
        "<model><name>box</name><sdf version='1.9'>model.sdf</sdf></model>");
  write(upload / "model.sdf", "<sdf version='1.9'><model name='box'/></sdf>");
  EXPECT_TRUE(client.UploadModel(upload.string(), id, {}, false));
  EXPECT_TRUE(client.UploadModel(upload.string(), id, {"X-Test: yes"}, true, "alice"));
  EXPECT_TRUE(client.PatchModel(id, {}));
  EXPECT_TRUE(client.PatchModel(id, {}, upload.string()));
  EXPECT_TRUE(client.DeleteUrl(gz::common::URI(http.url + "/1.0/alice/models/box", true), {}));
  EXPECT_TRUE(client.DeleteUrl(gz::common::URI(http.url + "/1.0/alice/worlds/arena", true), {}));
  EXPECT_FALSE(client.DeleteUrl(gz::common::URI("invalid"), {}));
  EXPECT_FALSE(client.DeleteUrl(gz::common::URI(http.url + "/1.0/alice/models/missing", true), {}));
  EXPECT_TRUE(client.Models(server(http)));
  EXPECT_TRUE(constant.Models(server(http)));
  EXPECT_TRUE(client.Models(id));
  EXPECT_TRUE(constant.Models(id));
  EXPECT_TRUE(client.Models(id, false));
  EXPECT_TRUE(constant.Models(id, false));
  EXPECT_TRUE(constant.Worlds(server(http)));
  EXPECT_TRUE(constant.Worlds(wid));
  CollectionIdentifier collection;
  collection.SetServer(server(http));
  collection.SetOwner("alice");
  collection.SetName("set");
  CollectionIdentifier parsedCollection;
  ASSERT_TRUE(client.ParseCollectionUrl(
      gz::common::URI(http.url + "/1.0/alice/collections/set", true), parsedCollection));
  EXPECT_EQ(collection, parsedCollection);
  EXPECT_TRUE(constant.Models(collection));
  EXPECT_TRUE(constant.Worlds(collection));
  EXPECT_EQ(path, fetchResourceWithClient(http.url + "/1.0/alice/models/box", client));
  EXPECT_TRUE(fetchResource("invalid").empty());
  FuelClient defaultClient;
  EXPECT_FALSE(defaultClient.Config().Servers().empty());
  FuelClient fresh(configuration(http, temporary.path / "fresh-cache"));
  EXPECT_TRUE(fresh.Worlds(wid));
  ModelIdentifier empty;
  EXPECT_FALSE(fresh.Models(empty, false));
}

TEST(FacadeCompatibility, OfflineListFallsBackToCachedModelsAndWorlds) {
  HttpFixture http;
  HttpFixture unavailableEndpoint(false);
  TempDirectory temporary;
  auto config = configuration(http, temporary.path);
  ServerConfig unavailable;
  unavailable.SetUrl(gz::common::URI(unavailableEndpoint.url, true));
  config.MutableServers().clear();
  config.AddServer(unavailable);
  LocalCache cache(&config);
  auto id = model(http);
  id.SetServer(unavailable);
  id.SetVersion(2);
  auto wid = world(http);
  wid.SetServer(unavailable);
  wid.SetVersion(2);
  ASSERT_TRUE(cache.SaveModel(id, modelZip(), false));
  ASSERT_TRUE(cache.SaveWorld(wid, worldZip(), false));
  FuelClient offline(config);
  auto models = offline.Models(unavailable);
  ASSERT_TRUE(models);
  EXPECT_EQ("box", models->Identification().Name());
  auto worlds = offline.Worlds(unavailable);
  ASSERT_TRUE(worlds);
  EXPECT_EQ("arena", worlds->Name());
}

TEST(FacadeCompatibility, IteratorOwnershipAndPrivateHelperContracts) {
  HttpFixture http;
  auto empty = ModelIterFactory::Create();
  ++empty;
  EXPECT_THROW(*empty, std::out_of_range);
  auto one = ModelIterFactory::Create(std::vector<ModelIdentifier>{model(http)});
  auto moved = std::move(one);
  EXPECT_FALSE(one);
  ASSERT_TRUE(moved);
  EXPECT_EQ(ResultType::FETCH_ERROR, moved->Fetch().Type());
  ++moved;
  EXPECT_FALSE(moved);
  ++moved;
  auto worlds = WorldIterFactory::Create(std::vector<WorldIdentifier>{world(http)});
  auto movedWorld = std::move(worlds);
  EXPECT_FALSE(worlds);
  EXPECT_EQ("arena", static_cast<WorldIdentifier>(movedWorld).Name());
  ++movedWorld;
  EXPECT_FALSE(movedWorld);
  ++movedWorld;
  EXPECT_THROW(*movedWorld, std::out_of_range);
  auto noWorlds = WorldIterFactory::Create();
  EXPECT_TRUE(static_cast<WorldIdentifier>(noWorlds).Name().empty());
}
#endif

TEST(FacadeCompatibility, ValueCopiesMovesReferencesAndEmptyJson) {
  Result original(ResultType::FETCH), copied(original), assigned;
  assigned = copied;
  EXPECT_TRUE(assigned);
  EXPECT_EQ(ResultType::FETCH, assigned.Type());
  Result moved(std::move(copied));
  EXPECT_TRUE(moved);
  assigned = Result(ResultType::PATCH);
  EXPECT_EQ(ResultType::PATCH, assigned.Type());
  ClientConfig config;
  const auto& agent = config.UserAgent();
  config.SetUserAgent("reference");
  EXPECT_EQ("reference", agent);
  ClientConfig copy(config), assignedConfig;
  assignedConfig = copy;
  EXPECT_EQ("reference", assignedConfig.UserAgent());
  assignedConfig.MutableServers().clear();
  EXPECT_TRUE(assignedConfig.Servers().empty());
  EXPECT_FALSE(copy.Servers().empty());
  WorldIdentifier world;
  world.SetName("World");
  world.SetOwner("Alice");
  world.SetPrivate(true);
  WorldIdentifier worldCopy(world), worldAssigned;
  worldAssigned = worldCopy;
  EXPECT_TRUE(worldAssigned.Private());
  EXPECT_FALSE(worldAssigned.AsPrettyString("prefix").empty());
  worldAssigned.Server().SetApiKey("mutated");
  EXPECT_EQ("mutated", worldAssigned.Server().ApiKey());
  EXPECT_NE("mutated", world.Server().ApiKey());
  CollectionIdentifier collection;
  collection.SetName("Set");
  collection.SetOwner("Alice");
  CollectionIdentifier collectionCopy(collection), collectionAssigned;
  collectionAssigned = collectionCopy;
  EXPECT_EQ(collection, collectionAssigned);
  EXPECT_FALSE(collectionAssigned.AsPrettyString("prefix").empty());
  EXPECT_FALSE(JSONParser::BuildModel(ModelIterFactory::Create()).empty());
  EXPECT_FALSE(JSONParser::BuildWorld(WorldIterFactory::Create()).empty());
  const auto parsed = JSONParser::ParseModels("[{\"name\":\"a\",\"owner\":\"b\"}]", ServerConfig());
  ASSERT_EQ(1u, parsed.size());
  EXPECT_EQ("a", parsed.front().Name());
  EXPECT_EQ(1u,
            JSONParser::ParseWorlds("[{\"name\":\"a\",\"owner\":\"b\"}]", ServerConfig()).size());
}
}  // namespace
