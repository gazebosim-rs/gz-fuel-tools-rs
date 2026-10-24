// SPDX-License-Identifier: Apache-2.0
fn main() {
    let (status, output) = gz_fuel_tools::cli::run(&std::env::args().skip(1).collect::<Vec<_>>());
    print!("{output}");
    std::process::exit(status);
}
