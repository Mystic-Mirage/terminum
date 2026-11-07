// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The shown pane of a window of several panes fills it (AC-13): terminum draws one pane, so a
//! sibling's share of the window (Claude Code's teammates take 70%) is only lost width. terminumd
//! zooms the pane, again when a split, a closed pane or a layout command took the zoom away; the
//! layout stays its maker's. The user's own tmux and a client attached by hand keep theirs.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::client::Look;
use super::sizing::{refused, take_zoom, users_pane};
use super::Page;
use crate::app::Shared;
use crate::screen::frame::Fields;
use crate::tmux::orphans::ours;
use crate::tmux::Server;

/// A zoom a program's layout command took away comes back at most this often.
const ZOOM_EVERY: Duration = Duration::from_millis(100);
/// A stream refused the zoom asks again after this.
const ZOOM_REFUSED: Duration = Duration::from_secs(1);

/// The line that zooms `pane`, as the fields last said: `resize-pane -Z` toggles, so a stale
/// `zoomed` unzooms and the stream's next frame zooms again. Never `if-shell`: one line, two answers.
pub(super) fn zoom_line(pane: &str, zoomed: bool) -> String {
    format!("{} -Z -t {pane}", if zoomed { "select-pane" } else { "resize-pane" })
}

/// Whether `lines` of `list-clients -F '#{client_flags}\t#{window_id}'` have a client terminumd did
/// not attach with `window` on its screen.
fn hand_attached(lines: &[String], window: &str) -> bool {
    lines.iter().filter_map(|l| l.split_once('\t')).any(|(flags, w)| w == window && !ours(flags))
}

/// Whether terminumd may zoom in `window`: not the user's own tmux (AC-89), nor a window a client
/// attached by hand has on its screen, whose user could unzoom it, only to have it zoomed again.
/// On the pane's client, as a shown pane's frame waits for it: the bulk client may be busy with history.
pub(super) async fn zoomable(server: &Server, pane: &str, window: &str) -> bool {
    if users_pane(server, pane) {
        return false;
    }
    let Some(client) = server.client(pane) else { return false };
    client.run("list-clients -F '#{client_flags}\t#{window_id}'".to_string()).await.is_ok_and(|lines| !hand_attached(&lines, window))
}

/// Whether a page whose look is `look` may take a window's zoom from another page holding it, whose
/// look is `holder` (None: no other page in sight holds it): a page in sight keeps it against any
/// page but an active one taking it from a page out of focus, so two pages never take it from
/// each other by themselves (AC-13).
pub(super) fn may_take(look: Look, holder: Option<Look>) -> bool {
    look != Look::Hidden && match holder {
        None | Some(Look::Hidden) => true,
        Some(Look::Away) => look == Look::On,
        Some(Look::On) => false,
    }
}

/// A stream's pace in zooming its pane.
#[derive(Default)]
pub struct ZoomPace {
    zoomed_at: Option<Instant>,
    refused_until: Option<Instant>,
}

/// The stream of a page in sight read `f` for its pane, not filling a window of several panes (a
/// split, a closed pane, a program's layout command): it zooms the pane, before any frame at the
/// narrow width (AC-13). This only zooms: the window's size stays the active page's. True: read
/// the pane again, no frame.
pub async fn zoom_shown(app: &Shared, page: &Arc<Page>, server: &Server, pane: &str, f: &Fields, pace: &mut ZoomPace) -> bool {
    if pace.refused_until.is_some_and(|t| t > Instant::now()) {
        return false;
    }
    if let Some(wait) = pace.zoomed_at.map(|t| ZOOM_EVERY.saturating_sub(t.elapsed())).filter(|w| !w.is_zero()) {
        tokio::time::sleep(wait).await;
        return true;
    }
    pace.refused_until = Some(Instant::now() + ZOOM_REFUSED);
    let Some(client) = server.client(pane) else { return false };
    if !zoomable(server, pane, &f.window).await || !take_zoom(app, page, server, &f.window) {
        return false;
    }
    if let Err(e) = client.run(zoom_line(pane, f.zoomed)).await {
        refused(page, &server.key(pane), f, "resize-pane", e);
        return false;
    }
    *pace = ZoomPace { zoomed_at: Some(Instant::now()), refused_until: None };
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-13: `resize-pane -Z` toggles, so the line follows what the fields said.
    #[test]
    fn a_zoom_moves_when_another_pane_has_it() {
        assert_eq!(zoom_line("%3", false), "resize-pane -Z -t %3");
        assert_eq!(zoom_line("%3", true), "select-pane -Z -t %3");
    }

    /// AC-13: terminumd's clients carry ignore-size; a `tmux -CC` by hand does not, control mode or not.
    #[test]
    fn a_client_attached_by_hand_holds_only_the_window_on_its_screen() {
        let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let ours = "attached,focused,control-mode,ignore-size,UTF-8\t@2";
        assert!(!hand_attached(&lines(&[ours]), "@2"));
        assert!(hand_attached(&lines(&[ours, "attached,focused,UTF-8\t@2"]), "@2"), "a plain client");
        assert!(hand_attached(&lines(&["attached,control-mode,UTF-8\t@2"]), "@2"), "tmux -CC by hand");
        assert!(!hand_attached(&lines(&["attached,focused,UTF-8\t@7"]), "@2"), "it looks at another window");
        assert!(!hand_attached(&[], "@2"));
    }

    /// AC-13: two pages never take a window's zoom from each other by themselves.
    #[test]
    fn a_zoom_goes_to_a_page_only_from_one_not_looking() {
        assert!(may_take(Look::Away, None), "no other page in sight holds it");
        assert!(may_take(Look::Away, Some(Look::Hidden)), "the holder is put away");
        assert!(may_take(Look::On, Some(Look::Away)), "the active page takes it from one out of focus");
        assert!(!may_take(Look::Away, Some(Look::Away)), "two pages out of focus: no fight");
        assert!(!may_take(Look::On, Some(Look::On)), "two active pages: no fight; typing moves it");
        assert!(!may_take(Look::Hidden, None), "a hidden page takes nothing");
    }
}
