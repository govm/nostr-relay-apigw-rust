use crate::ddb::Ddb;
use crate::message::Event;
use async_trait::async_trait;
use once_cell::sync::Lazy;

pub static HOOKS: Lazy<Hooks> = Lazy::new(Hooks::new);

#[async_trait]
pub trait Hook: Sync {
    async fn pre_event_write_hook(&self, _ev: &Event) {}
    async fn post_event_write_hook(&self, _ev: &Event) {}
}

pub struct Hooks {
    hooks: Vec<Box<dyn Hook + Sync + Send>>,
}

impl Hooks {
    pub fn new() -> Hooks {
        let hooks: Vec<Box<dyn Hook + Sync + Send>> = vec![
            Box::new(HookNIP2 {}),
            Box::new(HookNIP9 {}),
            Box::new(HookNIP16 {}),
        ];
        Hooks { hooks }
    }

    pub async fn pre_event_write_hook(&self, ev: &Event) {
        for hook in self.hooks.iter() {
            hook.pre_event_write_hook(ev).await;
        }
    }

    pub async fn post_event_write_hook(&self, ev: &Event) {
        for hook in self.hooks.iter() {
            hook.post_event_write_hook(ev).await;
        }
    }
}

struct HookNIP2 {}

#[async_trait]
impl Hook for HookNIP2 {
    async fn pre_event_write_hook(&self, ev: &Event) {
        let target_kinds = [3];

        if !target_kinds.contains(&ev.kind) {
            return;
        }
        println!("nip2 pre_event_write_hook");
        let ddb = Ddb::new().await;
        let pubkey = &ev.pubkey;

        if let Ok(evs) = ddb
            .get_event_by_pubkeys(
                [pubkey.to_string()].as_ref(),
                Some([3].to_vec()),
                None,
                None,
                None,
            )
            .await
        {
            let ids: Vec<String> = evs.iter().map(|ev| ev.id.to_string()).collect();
            if ids.is_empty() {
                return;
            }
            match ddb.delete_event_by_ids(ids).await {
                Ok(_) => (),
                Err(e) => println!("Hook_nip3 err:{e:?}"),
            }
        };
    }
}

struct HookNIP9 {}
#[async_trait]
impl Hook for HookNIP9 {
    async fn post_event_write_hook(&self, ev: &Event) {
        let target_kinds = [5];

        if !target_kinds.contains(&ev.kind) {
            return;
        }
        println!("nip9 post_event_write_hook");
        let ddb = Ddb::new().await;
        let pubkey = &ev.pubkey;
        let mut ids = vec![];

        for tag in ev.tags.iter() {
            if tag.len() >= 2 && tag[0] == "e" {
                ids.push(tag[1].clone())
            }
        }

        if let Ok(evs) = ddb.get_event_by_ids(&ids).await {
            let ids: Vec<String> = evs
                .iter()
                .filter_map(|ev| {
                    if ev.pubkey == *pubkey {
                        Some(ev.id.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            if ids.is_empty() {
                return;
            }
            match ddb.delete_event_by_ids(ids).await {
                Ok(_) => (),
                Err(e) => println!("Hook_nip9 err:{e:?}"),
            }
        };
    }
}

struct HookNIP16 {}
#[async_trait]
impl Hook for HookNIP16 {
    /// NIP-16 Replaceable Events
    async fn post_event_write_hook(&self, ev: &Event) {
        if !(10000 <= ev.kind && ev.kind < 20000) {
            return;
        }
        println!("nip16 post_event_write_hook");
        let ddb = Ddb::new().await;
        let pubkey = &ev.pubkey;

        if let Ok(evs) = ddb
            .get_event_by_pubkeys([pubkey.to_string()].as_ref(), None, None, None, None)
            .await
        {
            let evs: Vec<&Event> = evs
                .iter()
                .filter(|evx| ev.kind == evx.kind && ev.created_at > evx.created_at)
                .collect();
            if evs.is_empty() {
                return;
            }
            let ids = evs.iter().map(|e| e.id.to_string()).collect();
            match ddb.delete_event_by_ids(ids).await {
                Ok(_) => (),
                Err(e) => println!("Hook_nip16 err:{e:?}"),
            }
        };
    }
}
