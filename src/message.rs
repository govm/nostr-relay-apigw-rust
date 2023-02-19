/*

This file contains code derived from the following software.

nostr-rs-relay
https://github.com/scsibug/nostr-rs-relay

The MIT License (MIT)

Copyright (c) 2021 Greg Heartsfield

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.

*/

use crate::ddb::{QueryByIds, QueryByPubkeys, QueryPlan};
use once_cell::sync::Lazy;
use secp256k1::hashes::{sha256, Hash};
use secp256k1::{schnorr, Secp256k1, VerifyOnly, XOnlyPublicKey};
use serde::de::Unexpected;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use serde_json::value::Value;
use serde_json::Number;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

static SECP: Lazy<Secp256k1<VerifyOnly>> = Lazy::new(Secp256k1::verification_only);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Event {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

impl Event {
    pub fn to_canonical(&self) -> Option<String> {
        let mut v: Vec<Value> = vec![];

        let id = Number::from(0);
        v.push(serde_json::Value::Number(id));
        v.push(serde_json::Value::String(self.pubkey.clone()));
        let created_at = Number::from(self.created_at);
        v.push(serde_json::Value::Number(created_at));
        let kind = Number::from(self.kind);
        v.push(serde_json::Value::Number(kind));
        v.push(self.tag_to_canonical());
        v.push(serde_json::Value::String(self.content.clone()));
        serde_json::to_string(&Value::Array(v)).ok()
    }

    fn tag_to_canonical(&self) -> Value {
        let mut v = Vec::<Value>::new();
        for tag in &self.tags {
            let mut tv = Vec::<Value>::new();
            for e in tag.iter() {
                tv.push(serde_json::Value::String(e.clone()));
            }
            v.push(serde_json::Value::Array(tv));
        }
        serde_json::Value::Array(v)
    }

    pub fn digest(&self) -> sha256::Hash {
        sha256::Hash::hash(self.to_canonical().unwrap_or("".into()).as_bytes())
    }

    pub fn hex_digest(&self) -> String {
        let d = self.digest();
        format!("{d:x}")
    }

    pub fn validate(&self) -> Result<(), &str> {
        let digest = self.digest();
        let sig = schnorr::Signature::from_str(&self.sig).unwrap();
        if let Ok(msg) = secp256k1::Message::from_slice(digest.as_ref()) {
            if let Ok(pubkey) = XOnlyPublicKey::from_str(&self.pubkey) {
                SECP.verify_schnorr(&sig, &msg, &pubkey)
                    .map_err(|_| "EventInvalidSignature")
            } else {
                println!("client sent malformed pubkey");
                Err("EventMalformedPubkey")
            }
        } else {
            println!("error converting digest to secp256k1 message");
            Err("EventInvalidSignature")
        }
    }

    pub fn is_nip16_ephemeral(&self) -> bool {
        20000 <= self.kind && self.kind < 30000
    }
}

#[derive(Serialize, Deserialize)]
pub struct MessageContext {
    pub connection_id: String,
    pub endpoint: String,
    pub command: String,
    pub create_at: u64,
}

impl MessageContext {
    pub fn new(
        connection_id: &str,
        endpoint: &str,
        command: &str,
        create_at: u64,
    ) -> MessageContext {
        MessageContext {
            connection_id: connection_id.into(),
            endpoint: endpoint.into(),
            command: command.into(),
            create_at,
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Filter {
    pub ids: Option<Vec<String>>,
    authors: Option<Vec<String>>,
    kinds: Option<Vec<u64>>,
    tags: Option<HashMap<char, HashSet<String>>>,
    since: Option<u64>,
    until: Option<u64>,
    limit: Option<i32>,
}

impl Serialize for Filter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        if let Some(ids) = &self.ids {
            map.serialize_entry("ids", &ids)?;
        }
        if let Some(authors) = &self.authors {
            map.serialize_entry("authors", &authors)?;
        }
        if let Some(kinds) = &self.kinds {
            map.serialize_entry("kinds", kinds)?;
        }
        if let Some(until) = &self.until {
            map.serialize_entry("until", until)?;
        }
        if let Some(since) = &self.since {
            map.serialize_entry("since", since)?;
        }
        if let Some(limit) = &self.limit {
            map.serialize_entry("limit", limit)?;
        }
        if let Some(tags) = &self.tags {
            for (k, v) in tags {
                let vals: Vec<&String> = v.iter().collect();
                map.serialize_entry(&format!("#{k}"), &vals)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Filter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let received: Value = Deserialize::deserialize(deserializer)?;
        let filter = received.as_object().ok_or_else(|| {
            serde::de::Error::invalid_type(
                Unexpected::Other("filter is not an object"),
                &"a json object",
            )
        })?;
        let mut f = Filter {
            ids: None,
            authors: None,
            kinds: None,
            tags: None,
            since: None,
            until: None,
            limit: None,
        };
        let empty_string = "".into();
        let mut ts = None;
        for (key, val) in filter {
            if key == "ids" {
                let raw_ids: Option<Vec<String>> = Deserialize::deserialize(val).ok();
                if let Some(a) = raw_ids.as_ref() {
                    if a.contains(&empty_string) {
                        return Err(serde::de::Error::invalid_type(
                            Unexpected::Other("prefix matches must not be empty sytings"),
                            &"a json object",
                        ));
                    }
                }
                f.ids = raw_ids;
            } else if key == "kinds" {
                f.kinds = Deserialize::deserialize(val).ok();
            } else if key == "since" {
                f.since = Deserialize::deserialize(val).ok();
            } else if key == "until" {
                f.until = Deserialize::deserialize(val).ok();
            } else if key == "limit" {
                f.limit = Deserialize::deserialize(val).ok();
            } else if key == "authors" {
                let raw_authors: Option<Vec<String>> = Deserialize::deserialize(val).ok();
                if let Some(a) = raw_authors.as_ref() {
                    if a.contains(&empty_string) {
                        return Err(serde::de::Error::invalid_type(
                            Unexpected::Other("prefix matches must not be empty strings"),
                            &"a json object",
                        ));
                    }
                }
                f.authors = raw_authors;
            } else if key.starts_with('#') && key.len() > 1 && val.is_array() {
                if let Some(tag_search) = tag_search_char_from_filter(key) {
                    if ts.is_none() {
                        ts = Some(HashMap::new());
                    }
                    if let Some(m) = ts.as_mut() {
                        let tag_vals: Option<Vec<String>> = Deserialize::deserialize(val).ok();
                        if let Some(v) = tag_vals {
                            let hs = v.into_iter().collect::<HashSet<_>>();
                            m.insert(tag_search.to_owned(), hs);
                        }
                    };
                } else {
                    continue;
                }
            }
        }
        f.tags = ts;
        Ok(f)
    }
}

fn tag_search_char_from_filter(tagname: &str) -> Option<char> {
    let tagname_nohash = &tagname[1..];
    let mut tagnamechars = tagname_nohash.chars();
    let firstchar = tagnamechars.next();
    match firstchar {
        Some(_) => {
            if tagnamechars.next().is_none() {
                firstchar
            } else {
                None
            }
        }
        None => None,
    }
}

fn prefix_match(prefixes: &[String], target: &str) -> bool {
    for prefix in prefixes {
        if target.starts_with(prefix) {
            return true;
        }
    }

    false
}

impl Filter {
    fn ids_match(&self, event: &Event) -> bool {
        self.ids
            .as_ref()
            .map_or(true, |vs| prefix_match(vs, &event.id))
    }

    fn authors_match(&self, event: &Event) -> bool {
        self.authors
            .as_ref()
            .map_or(true, |vs| prefix_match(vs, &event.pubkey))
    }

    fn tag_match(&self, event: &Event) -> bool {
        if let Some(map) = &self.tags {
            for (key, val) in map.iter() {
                let mut tagmatch = false;
                for tag in &event.tags {
                    if tag[0].chars().next().unwrap() == *key
                        && tag[1..].iter().any(|v| val.contains(v))
                    {
                        tagmatch = true
                    }
                }
                if !tagmatch {
                    return false;
                }
            }
        }
        true
    }

    fn kind_match(&self, kind: u64) -> bool {
        self.kinds.as_ref().map_or(true, |ks| ks.contains(&kind))
    }

    pub fn event_match(&self, event: &Event) -> bool {
        self.ids_match(event)
            && self.since.map_or(true, |t| event.created_at > t)
            && self.until.map_or(true, |t| event.created_at < t)
            && self.kind_match(event.kind)
            && self.authors_match(event)
            && self.tag_match(event)
    }

    pub fn query_plan(&self) -> QueryPlan {
        if let Some(ids) = &self.ids {
            return QueryPlan::ByIds(QueryByIds::new(self, ids.to_vec()));
        }
        if let Some(authors) = &self.authors {
            return QueryPlan::ByPubkeys(QueryByPubkeys::new(
                self,
                authors.to_vec(),
                self.kinds.clone(),
                self.since,
                self.until,
                self.limit,
            ));
        }

        QueryPlan::NoPlan("invalid: we do not support this filter".to_string())
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum EventMsg {
    String(String),
    Event(Event),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EventCmd {
    pub cmd: String,
    pub event: Event,
}

impl EventCmd {
    pub fn new(cmd: &str, event: &Event) -> EventCmd {
        EventCmd {
            cmd: cmd.into(),
            event: event.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum ReqMsg {
    String(String),
    Filter(Filter),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReqCmd {
    pub cmd: String,
    pub subscription_id: String,
    pub filters: Vec<Filter>,
}

impl ReqCmd {
    pub fn new(cmd: &str, subscription_id: &str, filters: Vec<Filter>) -> ReqCmd {
        ReqCmd {
            cmd: cmd.into(),
            subscription_id: subscription_id.into(),
            filters,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum CloseMsg {
    String(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CloseCmd {
    pub cmd: String,
    pub subscription_id: String,
}

impl CloseCmd {
    pub fn new(cmd: &str, subscription_id: &str) -> CloseCmd {
        CloseCmd {
            cmd: cmd.into(),
            subscription_id: subscription_id.into(),
        }
    }
}

/// https://github.com/nostr-protocol/nips/blob/master/20.md
#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum CommandResult {
    String(String),
    Bool(bool),
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::Event;
    use super::Filter;

    fn build_event01() -> Event {
        Event {
            id: "87ae4ae2974e96e857856fe5f677d412df40cb331378fd1b20e0ed78910629a2".into(),
            pubkey: "98f4285bcb2cc65c3a66bd77ccffd2563ed3303e7e02a489c63a887fcd06bbe5".into(),
            created_at: 1676118868,
            kind: 1,
            tags: [].to_vec(),
            content: "hello!".into(),
            sig: "e9bfd020031ae702d5af21f029613d8a7957bfc269d5a8da36a79c2ff696f54db68e3ccd4111171f61335fa89369cbe96fa45b2a032061726a04afa157df32eb".into()
        }
    }

    fn build_event01_but_broken_sig() -> Event {
        Event {
            sig: "000fd020031ae702d5af21f029613d8a7957bfc269d5a8da36a79c2ff696f54db68e3ccd4111171f61335fa89369cbe96fa45b2a032061726a04afa157df32eb".into(),
            ..build_event01()
        }
    }

    #[test]
    fn event_to_canonical() {
        let ev = build_event01();
        let mut expect = r#"[0,
        "98f4285bcb2cc65c3a66bd77ccffd2563ed3303e7e02a489c63a887fcd06bbe5",
        1676118868,
        1,
        [],
        "hello!"]
        "#
        .to_string();
        expect.retain(|c| !c.is_whitespace());

        assert_eq!(expect, ev.to_canonical().unwrap());
    }

    #[test]
    fn event_to_digest() {
        let ev = build_event01();
        let digest = ev.hex_digest();

        assert_eq!(ev.id, digest);
    }

    #[test]
    fn event_validate() {
        let ev = build_event01();
        assert!(ev.validate().is_ok());

        let ev_broken = build_event01_but_broken_sig();
        assert!(ev_broken.validate().is_err());
    }

    fn build_filter01() -> Filter {
        let mut tags = HashMap::new();
        let mut tag_e = HashSet::new();
        tag_e.insert("id1".to_string());
        tag_e.insert("id2".to_string());
        let tag_p = HashSet::new();
        tags.insert('e', tag_e);
        tags.insert('p', tag_p);

        Filter {
            ids: Some(vec!["id1".into(), "id2".into()]),
            authors: Some(vec!["pub1".into(), "pub2".into()]),
            kinds: Some(vec![0]),
            tags: Some(tags),
            since: Some(1),
            until: Some(2),
            limit: Some(3),
        }
    }

    #[test]
    fn filter01() {
        let f = build_filter01();
        let fs = serde_json::to_string(&f).unwrap();
        let fsf: Filter = serde_json::from_str(&fs).unwrap();

        assert_eq!(f, fsf);
    }

    #[test]
    fn filter_match01() {
        let ev = build_event01();
        let fl = Filter {
            ids: Some(vec!["87ae4a".into()]),
            authors: None,
            kinds: None,
            tags: None,
            since: None,
            until: None,
            limit: None,
        };
        assert!(fl.event_match(&ev));

        let fl = Filter {
            ids: None,
            authors: Some(vec!["98f4".into()]),
            kinds: None,
            tags: None,
            since: None,
            until: None,
            limit: None,
        };
        assert!(fl.event_match(&ev));

        let fl = Filter {
            ids: None,
            authors: None,
            kinds: Some(vec![1]),
            tags: None,
            since: None,
            until: None,
            limit: None,
        };
        assert!(fl.event_match(&ev));

        let ev2 = Event {
            tags: vec![vec![
                "e".into(),
                "87ae4ae2974e96e857856fe5f677d412df40cb331378fd1b20e0ed78910629a2".into(),
                "relay".into(),
            ]],
            ..ev.clone()
        };
        let mut tags = HashMap::new();
        let mut tag_e = HashSet::new();
        tag_e
            .insert("87ae4ae2974e96e857856fe5f677d412df40cb331378fd1b20e0ed78910629a2".to_string());
        tags.insert('e', tag_e);
        let fl = Filter {
            ids: None,
            authors: None,
            kinds: None,
            tags: Some(tags),
            since: None,
            until: None,
            limit: None,
        };
        assert!(fl.event_match(&ev2));

        let fl = Filter {
            ids: None,
            authors: None,
            kinds: None,
            tags: None,
            since: Some(1676100000),
            until: None,
            limit: None,
        };
        assert!(fl.event_match(&ev));

        let fl = Filter {
            ids: None,
            authors: None,
            kinds: None,
            tags: None,
            since: None,
            until: Some(1676200000),
            limit: None,
        };
        assert!(fl.event_match(&ev));
    }
}
