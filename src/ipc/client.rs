use crate::error::{Error, Result};
use crate::ipc::{Event, Request, Response, read_line, write_line};
use crate::paths;
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{RecvHalf, SendHalf, Stream};
use std::io::BufReader;
use std::time::{Duration, Instant};

pub struct Client {
    rx: BufReader<RecvHalf>,
    tx: SendHalf,
}

fn socket_name() -> Result<interprocess::local_socket::Name<'static>> {
    let path = paths::socket_path();
    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, ToNsName};
        path.to_string_lossy()
            .into_owned()
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| Error::Ipc(e.to_string()))
    }
    #[cfg(not(windows))]
    {
        use interprocess::local_socket::{GenericFilePath, ToFsName};
        path.to_fs_name::<GenericFilePath>().map_err(|e| Error::Ipc(e.to_string()))
    }
}

pub(crate) fn listener_name() -> Result<interprocess::local_socket::Name<'static>> {
    socket_name()
}

impl Client {
    pub fn connect() -> Result<Client> {
        let stream = Stream::connect(socket_name()?).map_err(|e| Error::Ipc(format!("daemon not reachable: {e}")))?;
        let (rx, tx) = stream.split();
        Ok(Client { rx: BufReader::new(rx), tx })
    }

    /// Connect, retrying until `timeout` elapses.
    pub fn connect_within(timeout: Duration) -> Result<Client> {
        let start = Instant::now();
        loop {
            match Client::connect() {
                Ok(c) => return Ok(c),
                Err(e) if start.elapsed() < timeout => {
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e),
            }
        }
    }

    pub fn call(&mut self, req: &Request) -> Result<Response> {
        write_line(&mut self.tx, req).map_err(|e| Error::Ipc(e.to_string()))?;
        match read_line::<_, Response>(&mut self.rx).map_err(|e| Error::Ipc(e.to_string()))? {
            Some(r) => r.into_result(),
            None => Err(Error::Ipc("daemon closed the connection".into())),
        }
    }

    /// Turn this connection into an event stream.
    pub fn subscribe(mut self) -> Result<impl Iterator<Item = Event>> {
        self.call(&Request::Subscribe)?;
        let Client { mut rx, tx } = self;
        Ok(std::iter::from_fn(move || {
            let _keep_writer_open = &tx;
            read_line::<_, Event>(&mut rx).ok().flatten()
        }))
    }
}

/// One-shot request against the running daemon.
pub fn call(req: &Request) -> Result<Response> {
    Client::connect()?.call(req)
}

pub fn is_running() -> bool {
    Client::connect().is_ok()
}
