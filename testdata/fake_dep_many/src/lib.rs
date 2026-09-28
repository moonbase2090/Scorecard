// SPDX-License-Identifier: MPL-2.0
// Keep these cfg-off imports compile-clean while still triggering SCA findings.
#[cfg(any())]
use missing_one::Thing;
#[cfg(any())]
use missing_two::Thing;
#[cfg(any())]
use missing_three::Thing;
#[cfg(any())]
use missing_four::Thing;
#[cfg(any())]
use missing_five::Thing;
#[cfg(any())]
use missing_six::Thing;
#[cfg(any())]
use missing_seven::Thing;
#[cfg(any())]
use missing_eight::Thing;
#[cfg(any())]
use missing_nine::Thing;
#[cfg(any())]
use missing_ten::Thing;
#[cfg(any())]
use missing_eleven::Thing;
#[cfg(any())]
use missing_twelve::Thing;
#[cfg(any())]
use missing_thirteen::Thing;
#[cfg(any())]
use missing_fourteen::Thing;
#[cfg(any())]
use missing_fifteen::Thing;
#[cfg(any())]
use missing_sixteen::Thing;

pub fn sentinel() -> u32 {
    1
}
