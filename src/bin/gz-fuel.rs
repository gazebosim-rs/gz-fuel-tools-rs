// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Standalone command-line entry point for Gazebo Fuel resource operations.
fn main() {
    let (status, output) = gz_fuel_tools::cli::run(&std::env::args().skip(1).collect::<Vec<_>>());
    print!("{output}");
    std::process::exit(status);
}
