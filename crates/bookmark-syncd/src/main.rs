use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use directories::ProjectDirs;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::broadcast,
};
use tracing::{info, warn};

#[derive(Parser)]
#[command(about = "Local-only bookmark synchronization daemon")]
struct Args {
    /// Directory containing bookmarks.sqlite and daemon.sock.
    #[arg(long, env = "BOOKMARK_SYNC_STATE_DIR")]
    state_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Message {
    Register { browser: String },
    Operation { operation: Operation },
    Snapshot { operations: Vec<Operation> },
    Resync,
    Accepted { operation_id: String },
    Error { message: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Operation {
    id: String,
    #[serde(rename = "nodeId")]
    node_id: String,
    kind: OperationKind,
    title: Option<String>,
    url: Option<String>,
    #[serde(rename = "parentId")]
    parent_id: Option<String>,
    index: Option<i64>,
    #[serde(rename = "nodeType")]
    node_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum OperationKind {
    Create,
    Change,
    Move,
    Remove,
}

struct State {
    database: Mutex<Connection>,
    events: broadcast::Sender<(String, Operation)>,
}

fn state_dir(args: &Args) -> Result<PathBuf> {
    if let Some(path) = &args.state_dir {
        return Ok(path.clone());
    }
    let dirs = ProjectDirs::from("io", "bookmark-sync", "bookmark-sync")
        .context("could not determine a per-user state directory")?;
    Ok(dirs.data_local_dir().to_path_buf())
}

fn initialise_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "\
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        CREATE TABLE IF NOT EXISTS operations (
          id TEXT PRIMARY KEY NOT NULL,
          source TEXT NOT NULL,
          payload TEXT NOT NULL,
          applied_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE IF NOT EXISTS bookmarks (
          id TEXT PRIMARY KEY NOT NULL,
          node_type TEXT NOT NULL,
          title TEXT NOT NULL DEFAULT '',
          url TEXT,
          parent_id TEXT,
          position INTEGER,
          deleted INTEGER NOT NULL DEFAULT 0,
          revision INTEGER NOT NULL DEFAULT 0
        );
    ",
    )?;
    Ok(())
}

fn initialise_database(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path.join("bookmarks.sqlite"))?;
    initialise_schema(&connection)?;
    Ok(connection)
}

fn current_bookmarks(connection: &Connection) -> Result<Vec<Operation>> {
    let mut statement = connection.prepare(
        "\
      WITH RECURSIVE tree(id, node_type, title, url, parent_id, position, depth) AS (
        SELECT id, node_type, title, url, parent_id, position, 0
        FROM bookmarks WHERE deleted = 0 AND (parent_id LIKE 'root:%' OR parent_id IS NULL)
        UNION ALL
        SELECT child.id, child.node_type, child.title, child.url, child.parent_id, child.position, tree.depth + 1
        FROM bookmarks AS child JOIN tree ON child.parent_id = tree.id
        WHERE child.deleted = 0
      )
      SELECT id, node_type, title, url, parent_id, position FROM tree ORDER BY depth, parent_id, position, id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Operation {
            id: format!("state:{}", row.get::<_, String>(0)?),
            node_id: row.get(0)?,
            kind: OperationKind::Create,
            title: Some(row.get(2)?),
            url: row.get(3)?,
            parent_id: row.get(4)?,
            index: row.get(5)?,
            node_type: Some(row.get(1)?),
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn apply(connection: &mut Connection, source: &str, operation: &Operation) -> Result<bool> {
    let transaction = connection.transaction()?;
    let already_seen = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM operations WHERE id = ?1)",
        [&operation.id],
        |row| row.get::<_, i64>(0),
    )? != 0;
    if already_seen {
        transaction.commit()?;
        return Ok(false);
    }

    transaction.execute(
        "INSERT INTO operations (id, source, payload) VALUES (?1, ?2, ?3)",
        params![operation.id, source, serde_json::to_string(operation)?],
    )?;
    match operation.kind {
        OperationKind::Create => {
            let node_type = operation.node_type.as_deref().unwrap_or("bookmark");
            transaction.execute(
                "\
              INSERT INTO bookmarks (id, node_type, title, url, parent_id, position, deleted, revision)
              VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 1)
              ON CONFLICT(id) DO UPDATE SET
                node_type = excluded.node_type, title = excluded.title, url = excluded.url,
                parent_id = excluded.parent_id, position = excluded.position, deleted = 0,
                revision = bookmarks.revision + 1",
                params![
                    operation.node_id,
                    node_type,
                    operation.title.as_deref().unwrap_or(""),
                    operation.url,
                    operation.parent_id,
                    operation.index
                ],
            )?;
        }
        OperationKind::Change => {
            transaction.execute(
                "\
              UPDATE bookmarks SET title = COALESCE(?2, title), url = COALESCE(?3, url), revision = revision + 1
              WHERE id = ?1 AND deleted = 0",
                params![operation.node_id, operation.title, operation.url],
            )?;
        }
        OperationKind::Move => {
            transaction.execute(
                "\
              UPDATE bookmarks SET parent_id = ?2, position = ?3, revision = revision + 1
              WHERE id = ?1 AND deleted = 0",
                params![operation.node_id, operation.parent_id, operation.index],
            )?;
        }
        OperationKind::Remove => {
            transaction.execute(
                "UPDATE bookmarks SET deleted = 1, revision = revision + 1 WHERE id = ?1",
                [&operation.node_id],
            )?;
        }
    }
    transaction.commit()?;
    Ok(true)
}

async fn write_message(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: &Message,
) -> Result<()> {
    writer
        .write_all(serde_json::to_string(message)?.as_bytes())
        .await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn serve(stream: UnixStream, state: Arc<State>) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let Some(line) = lines.next_line().await? else {
        return Ok(());
    };
    let Message::Register { browser } =
        serde_json::from_str(&line).context("first client message must register")?
    else {
        bail!("first client message must be register");
    };
    if !matches!(browser.as_str(), "chrome" | "firefox") {
        bail!("unrecognized browser {browser:?}");
    }
    let mut events = state.events.subscribe();
    info!(%browser, "extension connected");
    let mut sent = HashSet::<String>::new();
    let current = current_bookmarks(&state.database.lock().expect("database mutex poisoned"))?;
    for operation in current {
        write_message(&mut write, &Message::Operation { operation }).await?;
    }

    loop {
        tokio::select! {
            input = lines.next_line() => match input? {
                Some(line) => match serde_json::from_str::<Message>(&line) {
                    Ok(Message::Operation { operation }) => {
                        let applied = apply(&mut state.database.lock().expect("database mutex poisoned"), &browser, &operation)?;
                        if applied {
                            sent.insert(operation.id.clone());
                            let _ = state.events.send((browser.clone(), operation.clone()));
                        }
                        write_message(&mut write, &Message::Accepted { operation_id: operation.id }).await?;
                    }
                    Ok(Message::Snapshot { operations }) => for operation in operations {
                        let applied = apply(&mut state.database.lock().expect("database mutex poisoned"), &browser, &operation)?;
                        if applied {
                            sent.insert(operation.id.clone());
                            let _ = state.events.send((browser.clone(), operation));
                        }
                    },
                    Ok(Message::Resync) => {
                        let current = current_bookmarks(&state.database.lock().expect("database mutex poisoned"))?;
                        for operation in current {
                            write_message(&mut write, &Message::Operation { operation }).await?;
                        }
                    }
                    Ok(_) => write_message(&mut write, &Message::Error { message: "unexpected client message".into() }).await?,
                    Err(error) => write_message(&mut write, &Message::Error { message: error.to_string() }).await?,
                },
                None => break,
            },
            event = events.recv() => match event {
                Ok((source, operation)) if source != browser && !sent.contains(&operation.id) => {
                    write_message(&mut write, &Message::Operation { operation }).await?;
                }
                Ok(_) => {},
                Err(broadcast::error::RecvError::Lagged(count)) => warn!(%browser, count, "extension lagged; it must reconcile its bookmark tree"),
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("bookmark_syncd=info")
        .init();
    let directory = state_dir(&Args::parse())?;
    fs::create_dir_all(&directory)?;
    fs::set_permissions(
        &directory,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    let socket = directory.join("daemon.sock");
    if socket.exists() {
        fs::remove_file(&socket).context("remove stale daemon socket")?;
    }
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    let (events, _) = broadcast::channel(1024);
    let state = Arc::new(State {
        database: Mutex::new(initialise_database(&directory)?),
        events,
    });
    info!(path = %socket.display(), "bookmark-sync daemon listening");
    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = serve(stream, state).await {
                warn!(%error, "client disconnected with error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation(id: &str, node_id: &str, kind: OperationKind) -> Operation {
        Operation {
            id: id.into(),
            node_id: node_id.into(),
            kind,
            title: Some("Example".into()),
            url: Some("https://example.test".into()),
            parent_id: Some("root:toolbar".into()),
            index: Some(0),
            node_type: Some("bookmark".into()),
        }
    }

    #[test]
    fn operations_are_idempotent_and_removals_are_tombstones() -> Result<()> {
        let mut database = Connection::open_in_memory()?;
        initialise_schema(&database)?;
        let create = operation("create-1", "node-1", OperationKind::Create);
        assert!(apply(&mut database, "chrome", &create)?);
        assert!(!apply(&mut database, "chrome", &create)?);
        assert_eq!(current_bookmarks(&database)?.len(), 1);

        let remove = operation("remove-1", "node-1", OperationKind::Remove);
        assert!(apply(&mut database, "firefox", &remove)?);
        let change = operation("change-1", "node-1", OperationKind::Change);
        assert!(apply(&mut database, "chrome", &change)?);
        assert!(current_bookmarks(&database)?.is_empty());
        Ok(())
    }
}
