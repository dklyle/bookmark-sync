use std::{
    env,
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    thread,
};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;

const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

fn socket_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("BOOKMARK_SYNC_SOCKET") {
        return Ok(path.into());
    }
    let dirs = ProjectDirs::from("io", "bookmark-sync", "bookmark-sync")
        .context("could not determine a per-user state directory")?;
    Ok(dirs.data_local_dir().join("daemon.sock"))
}

fn read_native(reader: &mut impl Read) -> Result<Option<Vec<u8>>> {
    let mut length = [0; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_MESSAGE_BYTES {
        bail!("native message exceeds {MAX_MESSAGE_BYTES} bytes");
    }
    let mut message = vec![0; length];
    reader.read_exact(&mut message)?;
    Ok(Some(message))
}

fn write_native(writer: &mut impl Write, message: &[u8]) -> Result<()> {
    let length: u32 = message.len().try_into().context("message too large")?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(message)?;
    writer.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let socket = UnixStream::connect(socket_path()?)
        .context("connect to bookmark-syncd; start the user service first")?;
    let mut socket_writer = BufWriter::new(socket.try_clone()?);
    let socket_reader = BufReader::new(socket);

    let output = thread::spawn(move || -> Result<()> {
        let mut output = BufWriter::new(io::stdout().lock());
        let mut input = socket_reader;
        let mut line = Vec::new();
        loop {
            line.clear();
            let bytes = input.read_until(b'\n', &mut line)?;
            if bytes == 0 {
                return Ok(());
            }
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            write_native(&mut output, &line)?;
        }
    });

    let mut input = BufReader::new(io::stdin().lock());
    while let Some(message) = read_native(&mut input)? {
        socket_writer.write_all(&message)?;
        socket_writer.write_all(b"\n")?;
        socket_writer.flush()?;
    }
    drop(socket_writer);
    output
        .join()
        .map_err(|_| anyhow::anyhow!("socket output thread panicked"))?
}
