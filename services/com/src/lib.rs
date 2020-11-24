#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use xous::{send_message, Error, CID};

pub fn power_off_soc(cid: CID) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::PowerOffSoc.into()).map(|_| ())
}

/*
pub fn elapsed_ms(cid: CID) -> Result<u64, Error> {
    let response = send_message(cid, api::Opcode::ElapsedMs.into())?;
    if let xous::Result::Scalar2(upper, lower) = response {
        Ok(upper as u64 | ((lower as u64) << 32))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn reset(cid: CID) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Reset.into()).map(|_| ())
}

pub fn sleep_ms(cid: CID, ms: usize) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SleepMs(ms).into()).map(|_| ())
}
*/