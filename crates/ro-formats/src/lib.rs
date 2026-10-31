//! Parsers for RO data formats. Pure: bytes in, structs out.

pub mod act;
pub mod bson;
pub mod gat;
pub mod gnd;
pub mod gr2;
pub(crate) mod igc;
pub mod image;
pub mod lub;
pub(crate) mod oodle0;
pub mod rsm;
pub mod rsw;
pub mod spr;
pub mod strm;

pub(crate) mod reader;
