#![feature(range_contains)]
#![no_std]

#[cfg(test)]
#[macro_use]
extern crate std;

extern crate byteorder;

pub mod wire;
pub mod interface;
