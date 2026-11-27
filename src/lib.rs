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
// Destructuring wire bytes into `[a, b, c, …]` is the clearest form for a protocol crate.
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::many_single_char_names
)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

#[cfg(feature = "std")]
extern crate std;

pub mod app;
pub mod compact;
pub mod crc;
mod error;
pub mod fragment;
pub mod json;
pub mod master;
#[cfg(feature = "oms")]
#[cfg_attr(docsrs, doc(cfg(feature = "oms")))]
pub mod oms;
pub mod profile;
pub mod record;
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub mod serial;
pub mod stick;
mod time;
mod value;
pub mod vif;
pub mod wired;
pub mod wireless;

pub use app::{
    Address, Application, Body, Header, LongHeader, Manufacturer, Medium, SecurityMode, ShortHeader,
};
pub use error::{Check, Error, Result};
pub use record::{DataField, Dib, Function, Record, Records};
pub use time::{Date, DateTime, Time};
pub use value::{Quantity, Unit, Value};
pub use vif::{Description, Kind, Modifier, Vib};
