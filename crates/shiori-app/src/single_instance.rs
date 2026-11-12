//! Only one copy of the app runs per user and data folder. A second start
//! connects to the first over a local socket (a Unix socket in the runtime
//! folder, a named pipe on Windows), asks it to show its window, and exits.

use std::path::Path;

use tokio::sync::mpsc::UnboundedReceiver;

/// What a later start asked the running app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Show,
}

#[derive(Debug)]
pub enum Instance {
    /// This is the only copy. Requests from later starts arrive here.
    First(UnboundedReceiver<Request>),
    /// Another copy runs and was asked to show itself.
    Other,
}

/// The socket name for a data folder, so that a copy started with a
/// different `SHIORI_HOME` is a separate app. FNV-1a, because the standard
/// hasher may change between Rust versions and two builds must agree.
fn name(data_folder: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in data_folder.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("shiori-{hash:016x}")
}

const SHOW: &[u8] = b"show\n";

/// Claims the instance for `data_folder`, or hands over to the copy that
/// already has it.
pub fn claim(data_folder: &Path) -> std::io::Result<Instance> {
    imp::claim(&name(data_folder))
}

#[cfg(unix)]
mod imp {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    use tokio::io::AsyncReadExt as _;
    use tokio::sync::mpsc::unbounded_channel;

    use super::{Instance, Request, SHOW};

    /// Runs the listener on its own thread with a small runtime, so that it
    /// works before and apart from the app's network thread.
    fn spawn_listener<F>(listen: impl FnOnce() -> F + Send + 'static) -> std::io::Result<()>
    where
        F: std::future::Future<Output = ()>,
    {
        std::thread::Builder::new()
            .name("shiori-instance".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(listen()),
                    Err(error) => tracing::error!("Single instance listener: {error}"),
                }
            })?;
        Ok(())
    }

    fn socket_path(name: &str) -> PathBuf {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("{name}.sock"))
    }

    pub fn claim(name: &str) -> std::io::Result<Instance> {
        let path = socket_path(name);
        if let Ok(mut stream) = UnixStream::connect(&path) {
            stream.write_all(SHOW)?;
            return Ok(Instance::Other);
        }
        // Nobody answered, so a file left there is from a copy that did not
        // exit cleanly.
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        let (sender, receiver) = unbounded_channel();
        spawn_listener(move || async move {
            let listener = match tokio::net::UnixListener::from_std(listener) {
                Ok(listener) => listener,
                Err(error) => return tracing::error!("Single instance listener: {error}"),
            };
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut message = Vec::new();
                let _ = (&mut stream).take(64).read_to_end(&mut message).await;
                if message == SHOW && sender.send(Request::Show).is_err() {
                    break;
                }
            }
        })?;
        Ok(Instance::First(receiver))
    }
}

#[cfg(windows)]
mod imp {
    use std::io::Write as _;

    use tokio::io::AsyncReadExt as _;
    use tokio::net::windows::named_pipe::ServerOptions;
    use tokio::sync::mpsc::unbounded_channel;

    use super::{Instance, Request, SHOW};

    fn tell_other(path: &str) -> bool {
        match std::fs::OpenOptions::new().write(true).open(path) {
            Ok(mut pipe) => pipe.write_all(SHOW).is_ok(),
            Err(_) => false,
        }
    }

    pub fn claim(name: &str) -> std::io::Result<Instance> {
        let path = format!(r"\\.\pipe\{name}");
        if tell_other(&path) {
            return Ok(Instance::Other);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()?;
        // Created here, before returning, so that of two copies starting at
        // the same moment only one can own the pipe name.
        let first = {
            let _guard = runtime.enter();
            ServerOptions::new().first_pipe_instance(true).create(&path)
        };
        let mut server = match first {
            Ok(server) => server,
            Err(error) => {
                return if tell_other(&path) {
                    Ok(Instance::Other)
                } else {
                    Err(error)
                };
            }
        };
        let (sender, receiver) = unbounded_channel();
        std::thread::Builder::new()
            .name("shiori-instance".into())
            .spawn(move || {
                runtime.block_on(async move {
                    loop {
                        if server.connect().await.is_ok() {
                            let mut message = Vec::new();
                            let _ = (&mut server).take(64).read_to_end(&mut message).await;
                            if message == SHOW && sender.send(Request::Show).is_err() {
                                break;
                            }
                        }
                        // Each connection uses up a pipe instance; make the next.
                        server = match ServerOptions::new().create(&path) {
                            Ok(server) => server,
                            Err(error) => {
                                tracing::error!("Single instance listener: {error}");
                                break;
                            }
                        };
                    }
                })
            })?;
        Ok(Instance::First(receiver))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn second_claim_reaches_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let Instance::First(mut requests) = claim(dir.path()).unwrap() else {
            panic!("the first claim should win");
        };
        assert!(matches!(claim(dir.path()).unwrap(), Instance::Other));
        let request = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), requests.recv()).await
            });
        assert_eq!(request.unwrap(), Some(Request::Show));
    }
}
