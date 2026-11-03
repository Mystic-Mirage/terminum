// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A capture of history rows against a private tmux server (`-L tm-test-hist-<pid>`, never the
//! user's): rows that scrolled off between the frame and the capture move the range with them,
//! so the page never shows other rows under these numbers (AC-02).

use std::time::Duration;

use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

use super::{capture, rows_at};
use crate::testutil::Tmux;
use crate::tmux::control::Control;

fn attach(s: &Tmux) -> Control {
    let mut cmd = Command::new("tmux");
    cmd.args(["-C", "-u", "-L", &s.socket, "-f", "/dev/null", "attach", "-f", "ignore-size", "-t", "main"]);
    let (ntx, nrx) = mpsc::unbounded_channel();
    let (dtx, drx) = oneshot::channel();
    std::mem::forget((nrx, drx));                       // notices and the end are not this test's
    Control::spawn(cmd, ntx, dtx).unwrap()
}

async fn history_size(c: &Control) -> i64 {
    c.run("display -p '#{history_size}'").await.unwrap()[0].trim().parse().unwrap()
}

/// The text of a captured row in the page format.
fn text(row: &serde_json::Value) -> String {
    row["r"].as_array().map(|runs| runs.iter().filter_map(|r| r[0].as_str()).collect::<String>()).unwrap_or_default().trim_end().to_string()
}

#[tokio::test]
async fn rows_that_scrolled_off_meanwhile_never_show_as_others() {
    let script = "seq -f L%04g 1 100; read x; seq -f L%04g 101 160; exec cat";
    let Some(s) = Tmux::start("hist", (80, 10), &format!("sh -c '{script}'")) else { return };
    let c = attach(&s);
    let pane = c.run("display -p '#{pane_id}'").await.unwrap().remove(0);
    let mut h = 0;
    for _ in 0..50 {
        h = history_size(&c).await;
        if h >= 90 { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // the page's numbering at that frame: nothing dropped, the screen's top is the history's size
    let (top, history) = (h, h);
    c.run(format!("send-keys -t {pane} Enter")).await.unwrap();
    for _ in 0..50 {
        if history_size(&c).await >= h + 60 { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(history_size(&c).await >= h + 60, "the pane went on: 60 more rows scrolled off");
    let rows = capture(&c, &pane, top, history, 2000, &[], 80, 84, 80).await.unwrap();
    let got: Vec<String> = rows.iter().map(text).collect();
    assert_eq!(got, ["L0081", "L0082", "L0083", "L0084"], "the rows asked for, not those 60 rows on");

    // a width tmux re-wrapped the history for: an error, not rows of another place
    s.read(&["resize-window", "-t", "main", "-x", "60", "-y", "10"]);
    let err = capture(&c, &pane, top, history, 2000, &[], 80, 84, 80).await.unwrap_err();
    assert!(err.contains("width"), "{err}");
    s.read(&["resize-window", "-t", "main", "-x", "80", "-y", "10"]);
    // a history near tmux's limit may have been trimmed: how far rows moved is not known, and
    // with no screen rows to find them by, an error
    let near = history_size(&c).await + 1;          // the history above the tenth tmux trims to
    let err = capture(&c, &pane, top, history, near, &[], 80, 84, 80).await.unwrap_err();
    assert!(err.contains("trimmed"), "{err}");
}

/// At tmux's history-limit the count falls by a tenth while rows still scroll off: the rows are
/// numbered by the last frame's screen rows, found where they moved (AC-02).
#[tokio::test]
async fn at_tmuxs_limit_rows_are_found_by_the_screen_they_were_above() {
    // 20 rows on, at a history-limit of 100: tmux drops a tenth on the way, the frame's rows stay
    let script = "seq -f L%04g 1 300; read x; seq -f L%04g 301 320; exec cat";
    let Some(s) = Tmux::start("hist-limit", (80, 10), "sleep 60") else { return };
    s.read(&["set-option", "-g", "history-limit", "100"]);
    s.read(&["new-window", "-t", "main", &format!("sh -c '{script}'")]);
    let c = attach(&s);
    let pane = c.run("display -p -t main:1 '#{pane_id}'").await.unwrap().remove(0);
    for _ in 0..50 {
        let shown = c.run(format!("capture-pane -p -t {pane}")).await.unwrap();
        if shown.iter().any(|l| l.contains("L0300")) { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // the frame: its screen rows, its history's count, and the 5 rows above the screen, numbered from 1000
    let shown = c.run(format!("capture-pane -p -e -J -t {pane}")).await.unwrap();
    let rows = crate::screen::rows::screen_rows(&shown.join("\n"), 80, 10);
    let screen: Vec<u64> = rows.iter().map(|r| crate::screen::history::hash(&crate::screen::rows::row_text(r))).collect();
    let history: i64 = c.run(format!("display -p -t {pane} '#{{history_size}}'")).await.unwrap()[0].trim().parse().unwrap();
    let above: Vec<String> = c.run(format!("capture-pane -p -t {pane} -S -5 -E -1")).await.unwrap();
    let top = 1000;
    c.run(format!("send-keys -t {pane} Enter")).await.unwrap();
    for _ in 0..50 {
        let shown = c.run(format!("capture-pane -p -t {pane}")).await.unwrap();
        if shown.iter().any(|l| l.contains("L0320")) { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let now: i64 = c.run(format!("display -p -t {pane} '#{{history_size}}'")).await.unwrap()[0].trim().parse().unwrap();
    assert!(now - history < 20, "tmux trimmed on the way: its count ({history} → {now}) does not say 21 rows moved");
    let got: Vec<String> = capture(&c, &pane, top, history, 100, &screen, top - 5, top, 80).await.unwrap().iter().map(text).collect();
    assert_eq!(got, above.iter().map(|l| l.trim_end().to_string()).collect::<Vec<_>>(), "the rows that were above the screen, though tmux trimmed meanwhile");
}

/// At tmux's limit, output that repeats itself (`yes`): the screen's rows are found at many moves,
/// so which one is not known: an error, never rows of another place (AC-02).
#[tokio::test]
async fn at_tmuxs_limit_rows_that_repeat_are_not_guessed() {
    let script = "yes same | head -300; read x; yes same | head -20; exec cat";
    let Some(s) = Tmux::start("hist-same", (80, 10), "sleep 60") else { return };
    s.read(&["set-option", "-g", "history-limit", "100"]);
    s.read(&["new-window", "-t", "main", &format!("sh -c '{script}'")]);
    let c = attach(&s);
    let pane = c.run("display -p -t main:1 '#{pane_id}'").await.unwrap().remove(0);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let shown = c.run(format!("capture-pane -p -e -J -t {pane}")).await.unwrap();
    let rows = crate::screen::rows::screen_rows(&shown.join("\n"), 80, 10);
    let screen: Vec<u64> = rows.iter().map(|r| crate::screen::history::hash(&crate::screen::rows::row_text(r))).collect();
    let history: i64 = c.run(format!("display -p -t {pane} '#{{history_size}}'")).await.unwrap()[0].trim().parse().unwrap();
    c.run(format!("send-keys -t {pane} Enter")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let got = capture(&c, &pane, 1000, history, 100, &screen, 995, 1000, 80).await;
    assert!(got.as_ref().is_err_and(|e| e.contains("not found")), "{got:?}");
}

/// At tmux's limit, a trim and as many rows again bring the count back to the frame's: the
/// screen's top rows say the rows moved, and they are found where they went (AC-02).
#[tokio::test]
async fn at_tmuxs_limit_a_count_come_back_still_finds_the_rows() {
    let script = "seq -f L%04g 1 295; read x; seq -f L%04g 296 304; exec cat";
    let Some(s) = Tmux::start("hist-again", (80, 10), "sleep 60") else { return };
    s.read(&["set-option", "-g", "history-limit", "100"]);
    s.read(&["new-window", "-t", "main", &format!("sh -c '{script}'")]);
    let c = attach(&s);
    let pane = c.run("display -p -t main:1 '#{pane_id}'").await.unwrap().remove(0);
    for _ in 0..50 {
        let shown = c.run(format!("capture-pane -p -t {pane}")).await.unwrap();
        if shown.iter().any(|l| l.contains("L0295")) { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let shown = c.run(format!("capture-pane -p -e -J -t {pane}")).await.unwrap();
    let rows = crate::screen::rows::screen_rows(&shown.join("\n"), 80, 10);
    let screen: Vec<u64> = rows.iter().map(|r| crate::screen::history::hash(&crate::screen::rows::row_text(r))).collect();
    let history: i64 = c.run(format!("display -p -t {pane} '#{{history_size}}'")).await.unwrap()[0].trim().parse().unwrap();
    let above: Vec<String> = c.run(format!("capture-pane -p -t {pane} -S -5 -E -1")).await.unwrap();
    c.run(format!("send-keys -t {pane} Enter")).await.unwrap();
    for _ in 0..50 {
        let shown = c.run(format!("capture-pane -p -t {pane}")).await.unwrap();
        if shown.iter().any(|l| l.contains("L0304")) { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let now: i64 = c.run(format!("display -p -t {pane} '#{{history_size}}'")).await.unwrap()[0].trim().parse().unwrap();
    assert_eq!(now, history, "tmux trimmed a tenth and 10 rows scrolled (the Enter and 9): the count came back ({history} → {now})");
    let got: Vec<String> = capture(&c, &pane, 1000, history, 100, &screen, 995, 1000, 80).await.unwrap().iter().map(text).collect();
    assert_eq!(got, above.iter().map(|l| l.trim_end().to_string()).collect::<Vec<_>>(), "the rows that were above the screen, not those 10 rows on");
}

/// At tmux's limit, the screen's top rows drawn over in place (a progress bar's redraw), nothing
/// scrolled (the Enter not echoed): they are found nowhere, and the count, unchanged, numbers the rows (AC-02).
#[tokio::test]
async fn at_tmuxs_limit_top_rows_drawn_over_in_place_keep_the_count() {
    let script = r"stty -echo; seq -f L%04g 1 295; read x; printf '\033[HNEW1\033[K\nNEW2\033[K\nNEW3\033[K'; exec cat";
    let Some(s) = Tmux::start("hist-redrawn", (80, 10), "sleep 60") else { return };
    s.read(&["set-option", "-g", "history-limit", "100"]);
    s.read(&["new-window", "-t", "main", &format!("sh -c \"{script}\"")]);
    let c = attach(&s);
    let pane = c.run("display -p -t main:1 '#{pane_id}'").await.unwrap().remove(0);
    for _ in 0..50 {
        let shown = c.run(format!("capture-pane -p -t {pane}")).await.unwrap();
        if shown.iter().any(|l| l.contains("L0295")) { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let shown = c.run(format!("capture-pane -p -e -J -t {pane}")).await.unwrap();
    let rows = crate::screen::rows::screen_rows(&shown.join("\n"), 80, 10);
    let screen: Vec<u64> = rows.iter().map(|r| crate::screen::history::hash(&crate::screen::rows::row_text(r))).collect();
    let history: i64 = c.run(format!("display -p -t {pane} '#{{history_size}}'")).await.unwrap()[0].trim().parse().unwrap();
    let above: Vec<String> = c.run(format!("capture-pane -p -t {pane} -S -5 -E -1")).await.unwrap();
    c.run(format!("send-keys -t {pane} Enter")).await.unwrap();
    for _ in 0..50 {
        let shown = c.run(format!("capture-pane -p -t {pane}")).await.unwrap();
        if shown.iter().any(|l| l.contains("NEW3")) { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let now: i64 = c.run(format!("display -p -t {pane} '#{{history_size}}'")).await.unwrap()[0].trim().parse().unwrap();
    assert!(history >= 90 && now == history, "at the limit, nothing scrolled ({history} → {now})");
    let got: Vec<String> = capture(&c, &pane, 1000, history, 100, &screen, 995, 1000, 80).await.unwrap().iter().map(text).collect();
    assert_eq!(got, above.iter().map(|l| l.trim_end().to_string()).collect::<Vec<_>>(), "the rows above the screen, as the count says");
}

/// Search reads the history and the screen while numbered output goes on (AC-34): every read
/// gives the rows that were at those numbers in the frame, never rows that scrolled since.
#[tokio::test]
async fn rows_read_for_search_while_output_goes_on_keep_their_numbers() {
    let script = "i=0; while :; do i=$((i+1)); echo N$i; done";
    let Some(s) = Tmux::start("hist-find", (80, 10), "sleep 60") else { return };
    s.read(&["set-option", "-g", "history-limit", "1000000"]);
    s.read(&["new-window", "-t", "main", &format!("sh -c '{script}'")]);
    let c = attach(&s);
    let pane = c.run("display -p -t main:1 '#{pane_id}'").await.unwrap().remove(0);
    while history_size(&c).await < 200 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (mut read, mut moved) = (0, 0);
    for _ in 0..20 {
        // the frame: history's count, the screen and the 50 rows above it, read at once
        let mut a = c.run_all(vec![format!("display -p -t {pane} '#{{history_size}}'"), format!("capture-pane -p -e -J -t {pane}"), format!("capture-pane -p -t {pane} -S -50 -E -1")]).await.into_iter();
        let top: i64 = a.next().unwrap().unwrap()[0].trim().parse().unwrap();
        let shown = a.next().unwrap().unwrap();
        let rows = crate::screen::rows::screen_rows(&shown.join("\n"), 80, 10);
        let screen: Vec<u64> = rows.iter().map(|r| crate::screen::history::hash(&crate::screen::rows::row_text(r))).collect();
        let mut want: Vec<String> = a.next().unwrap().unwrap().iter().map(|l| l.trim_end().to_string()).collect();
        want.extend(rows.iter().map(|r| crate::screen::rows::row_text(r).trim_end().to_string()));
        tokio::time::sleep(Duration::from_millis(30)).await;
        // the stale offsets alone (what search read before) would land on later rows
        let naive = c.run(format!("capture-pane -p -t {pane} -S -50 -E -1")).await.unwrap();
        if naive.first().map(|l| l.trim_end()) != want.first().map(String::as_str) {
            moved += 1;
        }
        let got = rows_at(&c, &pane, top, top, 1_000_000, &screen, top - 50, top + 10, 80).await.unwrap();
        let got: Vec<String> = got.iter().map(|r| r.as_ref().map(crate::screen::rows::row_text).unwrap_or_default().trim_end().to_string()).collect();
        // the screen's last rows may have been half written at the frame: the rows above them must match
        assert_eq!(got[..57], want[..57], "the rows at the frame's numbers");
        read += 1;
    }
    assert_eq!(read, 20);
    assert!(moved > 0, "the output went on between the frame and the read");
}
