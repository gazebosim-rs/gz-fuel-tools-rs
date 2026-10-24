// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
// Exercises exported C frontend hooks, stream capture, return values and metadata.
#include <gtest/gtest.h>

#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <gz/common/Console.hh>
#include <iostream>
#include <sstream>
#include <string>

#include "gz.hh"
namespace {
namespace fs = std::filesystem;
class CommandCompatibility : public ::testing::Test {
 protected:
  fs::path path;
  std::stringstream output, errors;
  std::streambuf *oldOutput{}, *oldErrors{};
  std::string previousCache;
  bool hadCache = false;
  int previousVerbosity = 0;
  void SetUp() override {
    path = fs::temp_directory_path() /
           ("fuel-command-" +
            std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    ASSERT_TRUE(fs::create_directories(path));
    if (const char* cache = std::getenv("GZ_FUEL_CACHE_PATH")) {
      hadCache = true;
      previousCache = cache;
    }
#ifdef _WIN32
    _putenv_s("GZ_FUEL_CACHE_PATH", path.string().c_str());
#else
    setenv("GZ_FUEL_CACHE_PATH", path.string().c_str(), 1);
#endif
    previousVerbosity = gz::common::Console::Verbosity();
    oldOutput = std::cout.rdbuf(output.rdbuf());
    oldErrors = std::cerr.rdbuf(errors.rdbuf());
  }
  void TearDown() override {
    std::cout.rdbuf(oldOutput);
    std::cerr.rdbuf(oldErrors);
    gz::common::Console::SetVerbosity(previousVerbosity);
#ifdef _WIN32
    _putenv_s("GZ_FUEL_CACHE_PATH", hadCache ? previousCache.c_str() : "");
#else
    if (hadCache)
      setenv("GZ_FUEL_CACHE_PATH", previousCache.c_str(), 1);
    else
      unsetenv("GZ_FUEL_CACHE_PATH");
#endif
    std::error_code ignored;
    fs::remove_all(path, ignored);
  }
  std::string write(const std::string& name, const std::string& text) {
    const auto file = path / name;
    std::ofstream(file) << text;
    return file.string();
  }
};
TEST_F(CommandCompatibility, VersionAndVerbosityHaveStableOwnershipAndNullHandling) {
  char* version = gzVersion();
  ASSERT_NE(version, nullptr);
  EXPECT_EQ(std::string(version), "12.0.0~pre2");
  std::free(version);
  cmdVerbosity("4");
  EXPECT_EQ(gz::common::Console::Verbosity(), 4);
  cmdVerbosity(nullptr);
  EXPECT_EQ(gz::common::Console::Verbosity(), 0);
}
TEST_F(CommandCompatibility, InvalidAndNullResourcesReportFailureWithoutThrowing) {
  EXPECT_EQ(listModels("fake_url", nullptr, nullptr, nullptr), 0);
  EXPECT_NE(output.str().find("Invalid URL"), std::string::npos);
  EXPECT_EQ(listWorlds("fake_url", "owner", "true", nullptr), 0);
  const auto missing = (path / "missing.yaml").string();
  EXPECT_EQ(listModels(nullptr, nullptr, nullptr, missing.c_str()), 0);
  EXPECT_EQ(listWorlds(nullptr, nullptr, nullptr, missing.c_str()), 0);
  EXPECT_EQ(downloadUrl(nullptr, nullptr, nullptr, nullptr, 0), 0);
  EXPECT_EQ(
      downloadUrl("http://example.invalid/wrong", nullptr, "Private-token: value", "models", 2), 0);
  EXPECT_EQ(deleteUrl(nullptr, nullptr), 0);
  EXPECT_EQ(deleteUrl("bad", "Private-token: value"), 0);
  EXPECT_EQ(editUrl(nullptr, nullptr, nullptr, nullptr), 0);
  EXPECT_EQ(editUrl("http://example.invalid/wrong", "Private-token: value", "true",
                    path.string().c_str()),
            0);
  EXPECT_EQ(upload(nullptr, nullptr, nullptr, nullptr, nullptr), 0);
  EXPECT_EQ(upload(path.string().c_str(), "bad", "Private-token: value", "1", "owner"), 0);
  EXPECT_EQ(upload(path.string().c_str(), nullptr, nullptr, "0", nullptr), 0);
  EXPECT_TRUE(errors.str().empty()) << errors.str();
}
TEST_F(CommandCompatibility, UpdatesOnEmptyCacheAndExplicitFiltersSucceed) {
  EXPECT_EQ(update(nullptr, nullptr, nullptr), 1);
  EXPECT_EQ(update("true", nullptr, "Private-token: value"), 1);
  EXPECT_EQ(update(nullptr, "1", nullptr), 1);
  EXPECT_EQ(update("1", "true", nullptr), 1);
}
TEST_F(CommandCompatibility, ConversionPreservesSelectedFileVersionAuthorsAndDependencies) {
  const auto config =
      write("model.config",
            "\n <?xml version='1.0'?><model><name>A &amp; B</name><version>1.0</version><sdf "
            "version='1.6'>older.sdf</sdf><sdf version='1.10'>model.sdf</sdf><description>A "
            "model</description><author><name>Alice</name><email>a@example.org</email></"
            "author><depend><model><uri>model://other</uri></model></depend></model>");
  ASSERT_EQ(config2Pbtxt(config.c_str()), 1);
  const auto metadata = output.str();
  EXPECT_NE(metadata.find("model.sdf"), std::string::npos);
  EXPECT_EQ(metadata.find("older.sdf"), std::string::npos);
  EXPECT_NE(metadata.find("minor: 10"), std::string::npos);
  EXPECT_NE(metadata.find("model://other"), std::string::npos);
  EXPECT_NE(metadata.find("Alice"), std::string::npos);
  output.str("");
  output.clear();
  const auto pb = write("metadata.pbtxt", metadata);
  ASSERT_EQ(pbtxt2Config(pb.c_str()), 1);
  const auto xml = output.str();
  EXPECT_NE(xml.find("version=\"1.10\""), std::string::npos);
  EXPECT_NE(xml.find("A &amp; B"), std::string::npos);
  EXPECT_NE(xml.find("model://other"), std::string::npos);
  EXPECT_TRUE(errors.str().empty()) << errors.str();
}
TEST_F(CommandCompatibility, ConversionErrorsUseErrorStreamAndFailureStatus) {
  EXPECT_EQ(config2Pbtxt(nullptr), 0);
  EXPECT_EQ(pbtxt2Config(nullptr), 0);
  const auto invalidConfig = write("bad.config", "<model/>");
  EXPECT_EQ(config2Pbtxt(invalidConfig.c_str()), 0);
  const auto invalidPb = write("bad.pbtxt", "model {");
  EXPECT_EQ(pbtxt2Config(invalidPb.c_str()), 0);
  EXPECT_FALSE(errors.str().empty());
  EXPECT_TRUE(output.str().empty());
}
}  // namespace
