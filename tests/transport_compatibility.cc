// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#include <gtest/gtest.h>

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <gz/fuel_tools.hh>
#include <stdexcept>
#include <string>
#include <thread>
#include <utility>
#include <vector>

#include "LocalCache.hh"
#ifndef _WIN32
#include <arpa/inet.h>
#include <sys/socket.h>
#include <unistd.h>

namespace {
using namespace gz::fuel_tools;
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
class BinaryServer {
 public:
  std::string url, input;
  explicit BinaryServer(std::string output) {
    socket_ = ::socket(AF_INET, SOCK_STREAM, 0);
    if (socket_ < 0) throw std::runtime_error("socket failed");
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (::bind(socket_, reinterpret_cast<sockaddr*>(&address), sizeof(address)) ||
        ::listen(socket_, 1))
      throw std::runtime_error("bind failed");
    socklen_t size = sizeof(address);
    ::getsockname(socket_, reinterpret_cast<sockaddr*>(&address), &size);
    url = "http://127.0.0.1:" + std::to_string(ntohs(address.sin_port));
    worker_ = std::thread([this, output = std::move(output)] {
      const int peer = ::accept(socket_, nullptr, nullptr);
      if (peer < 0) return;
      timeval timeout{10, 0};
      ::setsockopt(peer, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
      std::string message;
      for (;;) {
        char bytes[65536];
        const auto count = ::recv(peer, bytes, sizeof(bytes), 0);
        if (count <= 0) break;
        message.append(bytes, count);
        const auto end = message.find("\r\n\r\n");
        if (end == std::string::npos) continue;
        auto header = message.substr(0, end);
        std::transform(header.begin(), header.end(), header.begin(),
                       [](unsigned char c) { return std::tolower(c); });
        const auto length = header.find("content-length:");
        const auto expected =
            length == std::string::npos ? 0 : std::stoull(header.substr(length + 15));
        if (message.size() >= end + 4 + expected) {
          input = message.substr(end + 4);
          break;
        }
      }
      const std::string head =
          "HTTP/1.1 200 OK\r\nContent-Length: " + std::to_string(output.size()) +
          "\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n";
      const auto send = [peer](const std::string& bytes) {
        std::size_t offset = 0;
        while (offset < bytes.size()) {
          const auto count =
              ::send(peer, bytes.data() + offset, bytes.size() - offset, MSG_NOSIGNAL);
          if (count <= 0) break;
          offset += count;
        }
      };
      send(head);
      send(output);
      ::close(peer);
    });
  }
  void finish() {
    if (worker_.joinable()) worker_.join();
  }
  ~BinaryServer() {
    ::shutdown(socket_, SHUT_RDWR);
    ::close(socket_);
    finish();
  }

 private:
  int socket_;
  std::thread worker_;
};
TEST(TransportCompatibility, RawBodiesPreserveEveryByteInBothDirections) {
  std::string bytes(1024 * 1024, '\0');
  for (std::size_t i = 0; i < bytes.size(); ++i) bytes[i] = static_cast<char>(i);
  BinaryServer server(bytes);
  Rest rest;
  const auto response = rest.Request(HttpMethod::POST, server.url, "", "binary", {}, {}, bytes);
  server.finish();
  EXPECT_EQ(response.statusCode, 200);
  EXPECT_EQ(response.data, bytes);
  EXPECT_EQ(server.input, bytes);
  EXPECT_EQ(response.headers.at("Content-Type"), "application/octet-stream");
}
TEST(TransportCompatibility, CacheSaveAcceptsRawZipInput) {
  const auto root = std::filesystem::temp_directory_path() /
                    ("fuel-binary-cache-" +
                     std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
  std::filesystem::create_directories(root);
  struct Cleanup {
    std::filesystem::path path;
    ~Cleanup() {
      std::error_code ignored;
      std::filesystem::remove_all(path, ignored);
    }
  } cleanup{root};
  const std::string bytes("a\0\xff\x80\r\n", 6);
  const auto data = zip(
      {{"world.sdf", "<sdf version='1.9'><world name='world'/></sdf>"}, {"binary.asset", bytes}});
  ClientConfig config;
  config.SetCacheLocation((root / "cache").string());
  WorldIdentifier id;
  id.SetOwner("alice");
  id.SetName("world");
  id.SetVersion(1);
  LocalCache cache(&config);
  ASSERT_TRUE(cache.SaveWorld(id, data, false));
  std::ifstream file(std::filesystem::path(id.LocalPath()) / "binary.asset", std::ios::binary);
  const std::string saved((std::istreambuf_iterator<char>(file)), {});
  EXPECT_EQ(saved, bytes);
  EXPECT_FALSE(cache.SaveWorld(id, "invalid zip", true));
  ModelIdentifier model;
  model.SetOwner("alice");
  model.SetName("model");
  model.SetVersion(1);
  const auto modelData =
      zip({{"model.config", "<model><name>model</name><sdf version='1.9'>model.sdf</sdf></model>"},
           {"model.sdf", "<sdf version='1.9'><model name='model'/></sdf>"},
           {"binary.asset", bytes}});
  ASSERT_TRUE(cache.SaveModel(model, modelData, false));
  const auto cached = cache.MatchingModel(model);
  ASSERT_TRUE(cached);
  std::ifstream modelFile(std::filesystem::path(cached.PathToModel()) / "binary.asset",
                          std::ios::binary);
  const std::string modelSaved((std::istreambuf_iterator<char>(modelFile)), {});
  EXPECT_EQ(modelSaved, bytes);
}
}  // namespace
#endif
