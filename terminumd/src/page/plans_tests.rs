use serde_json::json;

use super::*;
use crate::plans::Plans;

const K: &str = "tmux:local:terminum:%1";

fn plan(p: &Plans, now: u64) {
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "1", "text": "alpha"}), now);
    p.hook(K, "claude", Some("s1"), &json!({"op": "add", "id": "2", "text": "beta"}), now);
    p.hook(K, "claude", Some("s1"), &json!({"op": "set", "id": "1", "status": "completed"}), now);
}

#[test]
fn row_gets_its_plan_and_loses_it() {
    let p = Plans::default();
    plan(&p, 100);
    let mut it = json!({"id": K, "plan": {"old": 1}});
    put(&mut it, p.row(K, false, 100));
    assert_eq!(it["plan"], json!({"done": 1, "total": 2, "pct": 50, "eta": null, "updated": 100, "stale": false, "partial": false}));
    put(&mut it, p.row("tmux:local:terminum:%9", false, 100));
    assert_eq!(it, json!({"id": K}));
}

#[test]
fn row_is_stale_only_while_working() {
    let p = Plans::default();
    plan(&p, 100);
    let late = 100 + 3600;
    let mut it = json!({"id": K});
    put(&mut it, p.row(K, false, late));
    assert_eq!(it["plan"]["stale"], false);
    put(&mut it, p.row(K, true, late));
    assert_eq!(it["plan"]["stale"], true);
}

#[test]
fn reply_with_plan_has_the_contract_fields() {
    let p = Plans::default();
    plan(&p, 100);
    let r = reply(K, p.details(K, false, 100));
    let pl = &r["plan"];
    assert_eq!(pl["items"], json!([{"text": "alpha", "status": "completed"}, {"text": "beta", "status": "pending"}]));
    assert_eq!((pl["done"].as_u64(), pl["total"].as_u64(), pl["pct"].as_u64()), (Some(1), Some(2), Some(50)));
    assert_eq!((pl["source"].as_str(), pl["agent"].as_str(), pl["partial"].as_bool()), (Some("tool"), Some("claude"), Some(false)));
    for k in ["title", "doing", "eta", "eta_given", "updated"] {
        assert!(pl.get(k).is_some(), "{k}");
    }
    assert!(r.get("notes").is_none(), "an agent's notes are its bookmarks (AC-98)");
}

#[test]
fn plannew_reaches_every_page_once_per_key() {
    let pages = super::super::Pages::default();
    let dir = crate::testutil::temp_root().join(format!("tm-plans-{}", std::process::id()));
    let (_a, mut ra) = pages.add(Default::default(), String::new(), crate::page::trace::Recorder::new(dir.clone())).unwrap();
    let (_b, mut rb) = pages.add(crate::inputs::Client { web: true, ..Default::default() }, String::new(), crate::page::trace::Recorder::new(dir)).unwrap();
    tell(&pages, &[K.into(), "tmux:local:terminum:%2".into()]);
    for rx in [&mut ra, &mut rb] {
        let got: Vec<Value> = std::iter::from_fn(|| rx.try_recv().ok()).map(|t| serde_json::from_str(&t).unwrap()).collect();
        assert_eq!(got, [json!({"t": "plannew", "key": K}), json!({"t": "plannew", "key": "tmux:local:terminum:%2"})]);
    }
    tell(&pages, &[]);
    assert!(ra.try_recv().is_err());
}

#[test]
fn a_plan_without_hook_events_says_hooks_false() {
    let d = |hooked: bool, plan: bool| {
        let mut v = json!({"t": "plan", "plan": if plan { json!({"title": "x"}) } else { Value::Null }});
        hooks(&mut v, hooked);
        v
    };
    assert_eq!(d(false, true)["plan"]["hooks"], false);
    assert!(d(true, true)["plan"].get("hooks").is_none(), "hooked: no field");
    assert!(d(false, false)["plan"].is_null(), "no plan: nothing to say");
}
