// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#include <gz/fuel_tools.hh>
#include <string>
static_assert(GZ_FUEL_TOOLS_MAJOR_VERSION == EXPECTED_MAJOR);
int main() {
  gz::fuel_tools::ClientConfig config;
  gz::fuel_tools::FuelClient client(config);
  return client.Config().UserAgent() == std::string("GazeboFuelTools-") + GZ_FUEL_TOOLS_VERSION_FULL
             ? 0
             : 1;
}
