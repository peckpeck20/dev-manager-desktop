use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use russh::client::{Handle, Msg};
use russh::{Channel, ChannelMsg, Disconnect, Sig};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use uuid::Uuid;
use vt100::Parser;

use crate::device_manager::Device;
use crate::session_manager::handler::ClientHandler;
use crate::session_manager::{Error, ErrorKind, Proc, Shell, ShellToken};

pub(crate) struct Connection {
    pub(crate) id: Uuid,
    pub(crate) device: Device,
    pub(crate) handle: AsyncMutex<Handle<ClientHandler>>,
    pub(crate) connections: Weak<Mutex<ConnectionsMap>>,
}

pub(crate) type ConnectionsMap = HashMap<String, Arc<Connection>>;

impl Connection {
    pub async fn exec(&self, command: &str, stdin: &Option<Vec<u8>>) -> Result<Vec<u8>, Error> {
        let mut ch = self.open_cmd_channel().await?;
        let id = ch.id();
        log::debug!("{id}: Exec {{ command: {command} }}");
        ch.exec(true, command).await?;
        if let Some(data) = stdin {
            let data = data.clone();
            ch.data(&*data).await?;
            ch.eof().await?;
        }
        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let mut status: Option<u32> = None;
        let mut eof: bool = false;
        loop {
            match ch.wait().await.ok_or(Error::new("empty message"))? {
                ChannelMsg::Data { data } => {
                    log::debug!("{id}: Data {{ data: {data:?} }}");
                    stdout.append(&mut data.to_vec());
                }
                ChannelMsg::ExtendedData { data, ext } => {
                    log::debug!("{id}: ExtendedData {{ data: {data:?}, ext: {ext} }}");
                    if ext == 1 {
                        stderr.append(&mut data.to_vec());
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => {
                    log::debug!("{id}: ExitStatus {{ exit_status: {exit_status} }}");
                    status = Some(exit_status);
                    if eof {
                        break;
                    }
                }
                ChannelMsg::Eof => {
                    log::debug!("{id}: Eof");
                    eof = true;
                    if status.is_some() {
                        break;
                    }
                }
                msg => log::debug!("{id}: {msg:?}"),
            }
        }
        let status = status.unwrap_or(0);
        if status != 0 {
            return Err(Error {
                message: format!("Command exited with non-zero return code {status}"),
                kind: ErrorKind::ExitStatus {
                    status,
                    output: stderr,
                },
            });
        }
        return Ok(stdout.to_vec());
    }

    pub async fn spawn(&self, command: &str) -> Result<Proc, Error> {
        let ch = self.open_cmd_channel().await?;
        return Ok(Proc {
            command: String::from(command),
            ch: AsyncMutex::new(Some(ch)),
        });
    }

    pub async fn shell(&self) -> Result<Shell, Error> {
        let connections = self
            .connections
            .upgrade()
            .expect("Connections should be available");
        let conn = connections
            .lock()
            .unwrap()
            .get(&self.device.name)
            .expect("Connection should be available")
            .clone();
        let mut ch = self.open_cmd_channel().await?;
        return Ok(Shell {
            token: ShellToken {
                connection_id: self.id,
                channel_id: ch.id().to_string(),
            },
            connection: Arc::downgrade(&conn),
            channel: AsyncMutex::new(Some(ch)),
            sender: AsyncMutex::default(),
            parser: Mutex::new(Parser::new(24, 80, 1000)),
            ready: Semaphore::new(0),
        });
    }

    pub(crate) fn remove_from_pool(&self) -> () {
        if let Some(connections) = self.connections.upgrade() {
            connections.lock().unwrap().remove(&self.device.name);
        }
    }

    async fn open_cmd_channel(&self) -> Result<Channel<Msg>, Error> {
        let result = self.handle.lock().await.channel_open_session().await;
        if let Err(e) = result {
            self.remove_from_pool();
            if let russh::Error::ChannelOpenFailure(_) = e {
                return Err(Error::reconnect());
            }
            return Err(e.into());
        }
        return Ok(result?);
    }

    pub(crate) fn new(
        id: Uuid,
        device: Device,
        handle: Handle<ClientHandler>,
        connections: Weak<Mutex<ConnectionsMap>>,
    ) -> Connection {
        log::info!("Created connection {} for device {}", id, device.name);
        return Connection {
            id,
            device,
            handle: AsyncMutex::new(handle),
            connections,
        };
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        log::info!(
            "Dropped connection {} for device {}",
            self.id,
            self.device.name
        );
    }
}
