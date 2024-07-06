use std::net::ToSocketAddrs;

use anyhow::Result;

#[derive(Debug, Default)]
pub struct Connection {
    pub nick: String,
}

impl Connection {
    pub fn connect<A: ToSocketAddrs>(nick: &str, addr: A) -> Result<Self> {
        todo!();
    }

    pub fn send(&self, msg: &str) -> Result<()> {
        // TODO: Send the message to the server
        todo!();
    }
}
