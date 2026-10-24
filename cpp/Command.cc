// SPDX-License-Identifier: Apache-2.0
#include <cstdlib>
#include <cstring>
#include <exception>
#include <gz/common/Console.hh>
#include <iostream>

#include "Bridge.hh"
#include "gz.hh"
namespace {
// The exported C hooks are called by FFI frontends which cannot unwind C++.
template <class Function>
int guarded(Function&& function) noexcept {
  try {
    return function();
  } catch (const std::exception& error) {
    std::cerr << "Fuel command failed: " << error.what() << '\n';
  } catch (...) {
    std::cerr << "Fuel command failed with an unknown error\n";
  }
  return 0;
}
std::string str(const char* p) { return p ? p : ""; }
int run(Json::Value args) {
  auto out = gz::fuel_tools::rs::call("command.run", args);
  std::cout << out["output"].asString();
  return out["ok"].asBool() ? 1 : 0;
}
int list(const char* u, const char* o, const char* r, const char* c, const char* k) {
  Json::Value a;
  a["action"] = "list";
  a["url"] = str(u);
  a["owner"] = str(o);
  a["raw"] = str(r);
  a["config_file"] = str(c);
  a["kind"] = k;
  return run(a);
}
}  // namespace
extern "C" char* gzVersion() { return strdup("12.0.0~pre2"); }
extern "C" void cmdVerbosity(const char* v) {
  gz::common::Console::SetVerbosity(v ? std::atoi(v) : 0);
}
extern "C" int listModels(const char* u, const char* o, const char* r, const char* c) {
  return guarded([&]() { return list(u, o, r, c, "models"); });
}
extern "C" int listWorlds(const char* u, const char* o, const char* r, const char* c) {
  return guarded([&]() { return list(u, o, r, c, "worlds"); });
}
extern "C" int downloadUrl(const char* u, const char* c, const char* h, const char* t, int jobs) {
  return guarded([&]() {
    Json::Value a;
    a["action"] = "download";
    a["url"] = str(u);
    a["config_file"] = str(c);
    a["header"] = str(h);
    a["kind"] = str(t);
    a["jobs"] = jobs;
    return run(a);
  });
}
extern "C" int upload(const char* p, const char* u, const char* h, const char* v, const char* o) {
  return guarded([&]() {
    Json::Value a;
    a["action"] = "upload";
    a["path"] = str(p);
    a["url"] = str(u);
    a["header"] = str(h);
    a["private"] = str(v);
    a["owner"] = str(o);
    return run(a);
  });
}
extern "C" int deleteUrl(const char* u, const char* h) {
  return guarded([&]() {
    Json::Value a;
    a["action"] = "delete";
    a["url"] = str(u);
    a["header"] = str(h);
    return run(a);
  });
}
extern "C" int editUrl(const char* u, const char* h, const char* v, const char* p) {
  return guarded([&]() {
    Json::Value a;
    a["action"] = "edit";
    a["url"] = str(u);
    a["header"] = str(h);
    a["private"] = str(v);
    a["path"] = str(p);
    return run(a);
  });
}
extern "C" int update(const char* m, const char* w, const char* h) {
  return guarded([&]() {
    Json::Value a;
    a["action"] = "update";
    a["only_models"] = str(m);
    a["only_worlds"] = str(w);
    a["header"] = str(h);
    return run(a);
  });
}
extern "C" int config2Pbtxt(const char* p) {
  return guarded([&]() {
    Json::Value a;
    a["path"] = str(p);
    auto out = gz::fuel_tools::rs::call("command.config_to_pbtxt", a);
    (out["ok"].asBool() ? std::cout : std::cerr) << out["output"].asString() << std::endl;
    return out["ok"].asBool() ? 1 : 0;
  });
}
extern "C" int pbtxt2Config(const char* p) {
  return guarded([&]() {
    Json::Value a;
    a["path"] = str(p);
    auto out = gz::fuel_tools::rs::call("command.pbtxt_to_config", a);
    (out["ok"].asBool() ? std::cout : std::cerr) << out["output"].asString() << std::endl;
    return out["ok"].asBool() ? 1 : 0;
  });
}
