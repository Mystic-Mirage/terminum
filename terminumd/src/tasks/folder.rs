// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Where a project's folder picker opens (`terminum-kanban.md` AC-139): its folder on its server, `~` read there,
//! so the Page never works out a home folder. The picker then lists through Files' own `/fb/api/ls`.

use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use serde_json::json;

use super::api::answer;
use crate::app::Shared;

pub fn routes() -> Router<Shared> {
    Router::new().route("/api/task-projects/folder", get(folder_at))
}

#[derive(Deserialize)]
struct FolderAt {
    #[serde(default)]
    host: String,
    #[serde(default)]
    dir: String,
}

/// Where `dir` is on a server whose home is `home`: as typed when absolute, else in the home folder.
fn folder_in(home: &str, dir: &str) -> String {
    let dir = dir.trim();
    if dir.starts_with('/') {
        return dir.to_string();
    }
    match dir.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
        None => home.to_string(),
    }
}

async fn folder_at(State(app): State<Shared>, Query(q): Query<FolderAt>) -> Response {
    let server = if q.host.is_empty() { Some(app.tmux.local.clone()) } else { app.tmux.host(&q.host).filter(|s| s.connected()) };
    let Some(server) = server else { return answer::<serde_json::Value>(Err(format!("The host {} is out of reach.", q.host))) };
    let home = if server.is_local() { Some(crate::config::home().display().to_string()) } else { server.home().await };
    answer(home.map(|h| json!({"path": folder_in(&h, &q.dir)})).ok_or_else(|| format!("The home folder on {} is not known.", server.name)))
}

#[cfg(test)]
mod tests {
    use super::folder_in;

    #[test]
    fn ac139_the_picker_opens_at_the_folder_or_home() {
        assert_eq!(folder_in("/Users/alex", "/srv/billing "), "/srv/billing", "absolute: as typed");
        assert_eq!(folder_in("/Users/alex/", "~/code/billing"), "/Users/alex/code/billing", "in the home folder");
        assert_eq!(folder_in("/home/alex", "~"), "/home/alex");
        assert_eq!(folder_in("/home/alex", ""), "/home/alex", "none set: home");
    }
}
