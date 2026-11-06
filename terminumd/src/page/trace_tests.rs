// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The latency trace (AC-38): what a page may record, the file and its limits, a key's stages
//! joined to the frame that shows it (ported from iterm-enhancer's test_web_trace.py).

use super::*;
use std::os::unix::fs::PermissionsExt;

fn logs(name: &str) -> PathBuf {
    let d = crate::testutil::temp_root().join(format!("tm-trace-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    d
}

fn records(rec: &Recorder) -> Vec<Value> {
    fs::read_to_string(rec.path().unwrap()).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn on(t: &mut Tracer) -> Vec<Value> {
    t.handle(&json!({"t": "trace", "on": true, "resume": false}), "v0.3.0")
}

fn later() {
    std::thread::sleep(Duration::from_millis(2));
}

#[test]
fn a_page_record_keeps_only_known_names_numbers_and_words() {
    let rec = clean(&json!({"ev": "echo", "k": 3, "cls": "char", "path": "input", "total": 12.3456, "via": "text",
        "data": "secret", "cur": "x", "rt": "NaN", "in": "5", "woken": "rm -rf", "buf": true, "n": 1e12}));
    assert_eq!(Value::Object(rec.unwrap()), json!({"ev": "echo", "k": 3, "cls": "char", "path": "input", "total": 12.35, "via": "text", "buf": true}));
    assert!(clean(&json!({"ev": "shell", "cmd": "ls"})).is_none());
    assert!(clean(&json!(["echo"])).is_none());
    let open = clean(&json!({"ev": "open", "sid": "tmux:devbox.example:default:%7", "first": 4})).unwrap();
    assert_eq!(Value::Object(open), json!({"ev": "open", "sid": "%7", "first": 4}), "the pane, never the host");
    let bad = clean(&json!({"ev": "open", "sid": "../../etc/passwd x"})).unwrap();
    assert_eq!(Value::Object(bad), json!({"ev": "open"}));
    assert_eq!(pane_of("%12"), Some("%12".into()));
    assert_eq!(pane_of("tmux:local:terminum:%x"), None);
}

#[test]
fn a_key_is_joined_to_the_frame_where_the_cursor_moved() {
    let rec = Recorder::new(logs("join"));
    let mut t = Tracer::new(rec.clone());
    let r = on(&mut t);
    assert_eq!((r[0]["on"].as_bool(), r[0]["left"].as_u64()), (Some(true), Some(TRACE_FOR.as_secs())));
    assert!(r[0]["file"].as_str().unwrap().starts_with("trace-"));
    t.opening("tmux:devbox.example:default:%7", true);
    let mut key = t.key(Some(7));
    t.step(&mut key, "to_tmux_ms", &[("via", json!("keys")), ("kind", json!("char"))]);
    t.typed(key);
    t.woken("key");
    later();
    t.captured(Instant::now());
    assert_eq!(t.frame(false), None, "a spinner's frame answers no key");
    later();
    t.woken("output");
    t.captured(Instant::now());
    let ks = t.frame(true).unwrap();
    let b = &ks[0];
    assert_eq!((b["k"].as_i64(), b["via"].as_str(), b["polls"].as_u64(), b["skip"].as_u64(), b["woken"].as_str(), b["moved"].as_bool()),
        (Some(7), Some("keys"), Some(1), Some(1), Some("output"), Some(true)));
    for n in ["to_tmux_ms", "until_change_ms", "capture_ms", "encode_ms", "total_ms"] {
        assert!(b[n].is_f64(), "{n}: {b}");
    }
    assert_eq!(t.frame(true), None, "a key is answered once");
    let echo = json!({"ev": "echo", "k": 7, "total": 40, "data": "a"});
    t.handle(&json!({"t": "trace", "ev": [echo, echo, echo, "junk"]}), "v0.3.0");
    let recs = records(&rec);
    let evs: Vec<&str> = recs.iter().map(|r| r["ev"].as_str().unwrap()).collect();
    assert_eq!(evs, vec!["start", "open", "key", "echo", "echo", "echo"]);
    assert_eq!((recs[1]["sid"].as_str(), recs[1]["mode"].as_str()), (Some("%7"), Some("remote")));
    assert_eq!((recs[2]["kind"].as_str(), recs[2]["src"].as_str()), (Some("char"), Some("terminumd")));
    assert_eq!((recs[3]["src"].as_str(), recs[3]["k"].as_i64(), recs[3]["mode"].as_str()), (Some("page"), Some(7), Some("remote")));
    assert!(recs.iter().all(|r| r["at"].as_u64().is_some()));
    let text = fs::read_to_string(rec.path().unwrap()).unwrap();
    assert!(!text.contains("data") && !text.contains("devbox"), "{text}");
    assert_eq!(fs::metadata(rec.path().unwrap()).unwrap().permissions().mode() & 0o777, 0o600);
    rec.stop();
}

#[test]
fn the_pages_own_trace_gets_the_stages_and_nothing_is_written() {
    let dir = logs("stages");
    let mut t = Tracer::new(Recorder::new(dir.clone()));
    assert_eq!(t.handle(&json!({"t": "trace", "stages": true}), "x"), vec![json!({"t": "trace", "stages": true})]);
    let mut key = t.key(Some(2));
    t.step(&mut key, "to_tmux_ms", &[]);
    t.typed(key);
    later();
    t.captured(Instant::now());
    let b = &t.frame(true).unwrap()[0];
    assert_eq!(b["k"], 2);
    assert!(b["total_ms"].as_f64().unwrap() >= 0.0);
    assert!(!dir.exists(), "no file");
    assert_eq!(t.handle(&json!({"t": "trace", "stages": "yes"}), "x"), vec![json!({"t": "trace", "stages": false})]);
    assert!(t.key(Some(3)).is_none());
}

#[test]
fn off_nothing_is_recorded_and_the_messages_carry_nothing() {
    let dir = logs("off");
    let mut t = Tracer::new(Recorder::new(dir.clone()));
    assert!(t.key(Some(1)).is_none());
    t.captured(Instant::now());
    assert_eq!(t.frame(true), None);
    t.handle(&json!({"t": "trace", "ev": [{"ev": "ping", "n": 1, "rtt": 3}]}), "x");
    assert!(t.tick().is_empty());
    assert!(!t.probe_due());
    assert!(!dir.exists());
    assert_eq!(t.handle(&json!({"t": "trace", "ping": 5}), "x"), vec![json!({"t": "trace", "pong": 5})]);
    assert!(t.handle(&json!({"t": "trace", "ping": "<b>"}), "x").is_empty());
}

#[test]
fn the_trace_ends_on_terminumds_clock_and_a_reconnect_after_it_is_told() {
    let rec = Recorder::with_limits(logs("clock"), MAX_BYTES, Duration::from_millis(30));
    let mut t = Tracer::new(rec.clone());
    on(&mut t);
    assert!(rec.on() && t.live());
    std::thread::sleep(Duration::from_millis(40));
    assert!(!rec.on());
    rec.note("poll", json!({"ms": 3}));
    assert_eq!(t.tick(), vec![json!({"t": "trace", "on": false})], "the page is told once");
    assert!(t.tick().is_empty());
    let mut again = Tracer::new(rec.clone());
    assert_eq!(again.handle(&json!({"t": "trace", "on": true, "resume": true}), "x"), vec![json!({"t": "trace", "on": false})]);
    assert_eq!(records(&rec).len(), 1, "only the start record");
}

/// A disk that is full.
struct Full;

impl Write for Full {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from_raw_os_error(28))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_second_page_joins_the_trace_and_a_failed_write_only_ends_it() {
    let rec = Recorder::new(logs("join2"));
    let mut first = Tracer::new(rec.clone());
    on(&mut first);
    let path = rec.path();
    let mut second = Tracer::new(rec.clone());
    on(&mut second);
    assert_eq!(rec.path(), path, "joined, not restarted");
    let key = second.key(Some(1));
    second.typed(key);
    later();
    second.captured(Instant::now());
    rec.inner.lock().file = Some(Box::new(Full));
    assert!(second.frame(true).is_some(), "the frame is still sent");
    assert!(!rec.on());
    assert_eq!(first.tick(), vec![json!({"t": "trace", "on": false})]);
    rec.note("poll", json!({"ms": 1}));
}

#[test]
fn a_key_typed_during_a_capture_waits_for_the_next_and_the_first_cause_counts() {
    let rec = Recorder::new(logs("during"));
    let mut t = Tracer::new(rec.clone());
    on(&mut t);
    let began = Instant::now();
    later();
    let key = t.key(Some(1));
    t.typed(key);
    t.woken("key");
    t.woken("output");
    t.captured(began);
    assert_eq!(t.frame(true), None, "this capture began before the key arrived");
    later();
    t.captured(Instant::now());
    let b = &t.frame(true).unwrap()[0];
    assert_eq!((b["k"].as_i64(), b["polls"].as_u64(), b["woken"].as_str()), (Some(1), Some(0), Some("next")));
    t.woken("poll");
    t.woken("key");
    t.captured(Instant::now());
    assert_eq!(t.read_woke, "poll");
    t.woken("bogus");
    t.captured(Instant::now());
    assert_eq!(t.read_woke, "next", "unknown causes are not recorded");
    rec.stop();
}

#[test]
fn the_file_stops_at_its_size_cap_and_keeps_the_newest_ten() {
    let dir = logs("cap");
    let rec = Recorder::with_limits(dir.clone(), 300, TRACE_FOR);
    rec.start().unwrap();
    for _ in 0..20 {
        rec.note("lag", json!({"ms": 25.0}));
    }
    assert!(!rec.on());
    assert!(fs::metadata(rec.path().unwrap()).unwrap().len() <= 300);
    fs::remove_file(rec.path().unwrap()).unwrap(); // the next start may fall in the same second
    for i in 0..12 {
        fs::write(dir.join(format!("trace-2026010{}-0000{:02}.jsonl", i % 9, i)), "").unwrap();
    }
    fs::write(dir.join("terminumd.log"), "").unwrap();
    rec.start().unwrap();
    let left: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
    assert_eq!(left.iter().filter(|n| n.starts_with("trace-")).count(), KEEP_FILES);
    assert!(left.contains(&"terminumd.log".to_string()), "other files stay");
    rec.stop();
}

#[test]
fn slow_work_and_probes_are_noted_only_while_recording() {
    let rec = Recorder::new(logs("slow"));
    let mut t = Tracer::new(rec.clone());
    let long_ago = Instant::now() - Duration::from_millis(50);
    t.handled("in", long_ago);
    on(&mut t);
    t.handled("in", long_ago);
    t.handled("rename", long_ago);
    t.handled("trace", long_ago);
    t.handled("more", Instant::now());
    t.sent("screen", 412, long_ago);
    t.hist("prepend", 1000, long_ago);
    t.probed(Ok(Duration::from_micros(700)));
    t.probed(Err("Timeout"));
    assert!(!t.probe_due(), "not before 6 s");
    t.probed = Instant::now() - PROBE_EVERY;
    assert!(t.probe_due());
    assert!(!t.probe_due());
    let recs = records(&rec);
    let got: Vec<(String, String)> = recs.iter().skip(1)
        .map(|r| (r["ev"].as_str().unwrap().into(), r.get("t").or(r.get("mode")).or(r.get("error")).map_or(String::new(), |v| v.to_string())))
        .collect();
    assert_eq!(got, vec![
        ("msg".into(), "\"in\"".into()), ("msg".into(), "\"other\"".into()), ("tx".into(), "\"screen\"".into()),
        ("hist".into(), "\"prepend\"".into()), ("tmux".into(), String::new()), ("tmux".into(), "\"Timeout\"".into()),
    ]);
    assert_eq!(recs[5]["ms"], 0.7);
    rec.stop();
}

#[tokio::test]
async fn the_lag_watch_closes_the_file_when_the_time_is_up() {
    let rec = Recorder::with_limits(logs("watch"), MAX_BYTES, Duration::from_millis(150));
    rec.start().unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(rec.inner.lock().file.is_none(), "closed by the watch");
}
