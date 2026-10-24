// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#pragma once
namespace Json {
class Value;
}
namespace gz::fuel_tools::rs {
// Single-owner handle; shared iterator cursor lifetime stays in its private PIMPL.
class Pager {
 public:
  explicit Pager(const Json::Value&);
  ~Pager();
  Pager(const Pager&) = delete;
  Pager& operator=(const Pager&) = delete;
  Json::Value Next();

 private:
  void* handle;
};
}  // namespace gz::fuel_tools::rs
