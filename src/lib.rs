#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
// Every byte this crate reads comes from a meter, a bus or a radio before anything about
// it is known, so the ways a decoder can abort the process are denied outright.
#![deny(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::todo,
    clippy::unimplemented,
    clippy::indexing_slicing,
    clippy::exit
)]
#![warn(clippy::pedantic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing))]
#![allow(clippy::module_name_repetitions, clippy::must_use_candidate, clippy::missing_errors_doc)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

#[cfg(feature = "std")]
extern crate std;

pub mod app;
pub mod crc;
mod error;
pub mod record;
mod value;
pub mod wired;
pub mod wireless;

pub use app::{Application, Body, Header, LongHeader, Manufacturer, Medium, SecurityMode, ShortHeader};
pub use error::{Error, Result};
pub use record::{DataField, Dib, Function, Record, Records, Vib};
pub use value::{Date, DateTime, Quantity, Unit, Value};
