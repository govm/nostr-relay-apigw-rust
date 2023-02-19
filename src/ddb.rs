use aws_sdk_dynamodb::{
    model::{AttributeValue, DeleteRequest, KeysAndAttributes, PutRequest, WriteRequest},
    Client,
};
use std::collections::HashMap;
use std::time::SystemTime;
use tokio_stream::StreamExt;

use crate::message::{Event, Filter};

pub struct Ddb {
    client: Client,
}

impl Ddb {
    pub async fn new() -> Ddb {
        let config = aws_config::load_from_env().await;
        let client = Client::new(&config);

        Ddb { client }
    }

    pub async fn write_event(
        &self,
        ev: &Event,
    ) -> Result<
        aws_sdk_dynamodb::output::BatchWriteItemOutput,
        aws_sdk_dynamodb::types::SdkError<aws_sdk_dynamodb::error::BatchWriteItemError>,
    > {
        let table = std::env::var("NOSTR_EVENT_TABLE").unwrap();
        let ttl: i64 = std::env::var("NOSTR_EVENT_TTL").unwrap().parse().unwrap();
        let ttl = ev.created_at as i64 + ttl;
        let id = &ev.id;

        let mut wrs = Vec::<WriteRequest>::new();

        let mut data = vec![
            (
                "pubkey".to_string(),
                AttributeValue::S(ev.pubkey.to_string()),
            ),
            (
                "created_at".to_string(),
                AttributeValue::N(ev.created_at.to_string()),
            ),
            ("kind".to_string(), AttributeValue::N(ev.kind.to_string())),
            (
                "content".to_string(),
                AttributeValue::S(ev.content.to_string()),
            ),
        ];

        for tag in ev.tags.iter() {
            let k = &tag[0];
            let v = tag[1..]
                .iter()
                .map(|v| AttributeValue::S(v.clone()))
                .collect();
            let tag_name = format!("tag_{k}");

            data.push((tag_name.to_string(), AttributeValue::L(v)));
        }

        data.push((
            "json".to_string(),
            AttributeValue::S(serde_json::to_string(ev).unwrap()),
        ));

        wrs.push(write_request(
            id,
            "event",
            AttributeValue::S("event".to_string()),
            Some(data),
            ttl,
        ));

        self.client
            .batch_write_item()
            .request_items(table, wrs)
            .send()
            .await
    }

    pub async fn write_subscription(
        &self,
        conn_id: &str,
        sub_id: &str,
        filters: &[Filter],
    ) -> Result<
        aws_sdk_dynamodb::output::BatchWriteItemOutput,
        aws_sdk_dynamodb::types::SdkError<aws_sdk_dynamodb::error::BatchWriteItemError>,
    > {
        let table = std::env::var("NOSTR_SUBSCRIPTION_TABLE").unwrap();
        let ttl: i64 = std::env::var("NOSTR_SUBSCRIPTION_TTL")
            .unwrap()
            .parse()
            .unwrap();
        let ttl = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + ttl;
        let id = sub_id;
        let mut wrs = Vec::<WriteRequest>::new();
        let fs = filters
            .iter()
            .map(|f| AttributeValue::S(serde_json::to_string(f).unwrap()))
            .collect();

        wrs.push(write_request(
            id,
            "conn_id",
            AttributeValue::S(conn_id.to_string()),
            Some(vec![("filters".to_string(), AttributeValue::L(fs))]),
            ttl,
        ));

        self.client
            .batch_write_item()
            .request_items(table, wrs)
            .send()
            .await
    }

    pub async fn delete_subscriptions(
        &self,
        sub_ids: Vec<String>,
    ) -> Result<
        aws_sdk_dynamodb::output::BatchWriteItemOutput,
        aws_sdk_dynamodb::types::SdkError<aws_sdk_dynamodb::error::BatchWriteItemError>,
    > {
        let table = std::env::var("NOSTR_SUBSCRIPTION_TABLE").unwrap();
        let mut wrs = Vec::<WriteRequest>::new();

        for sub_id in sub_ids {
            let id = sub_id;
            wrs.push(delete_request(&id, "conn_id"));
        }

        self.client
            .batch_write_item()
            .request_items(table, wrs)
            .send()
            .await
    }

    pub async fn close_connection(
        &self,
        conn_id: &str,
    ) -> Result<
        aws_sdk_dynamodb::output::BatchWriteItemOutput,
        aws_sdk_dynamodb::types::SdkError<aws_sdk_dynamodb::error::BatchWriteItemError>,
    > {
        let table = std::env::var("NOSTR_SUBSCRIPTION_TABLE").unwrap();
        let mut sub_ids = Vec::<String>::new();

        let items: Result<Vec<_>, _> = self
            .client
            .query()
            .table_name(&table)
            .index_name("value-id-index")
            .key_condition_expression("#value = :conn_id")
            .expression_attribute_names("#value", "value")
            .expression_attribute_values(":conn_id", AttributeValue::S(conn_id.to_string()))
            .into_paginator()
            .items()
            .send()
            .collect()
            .await;

        if let Ok(items) = items {
            for item in items {
                if let Some(sub_id) = item.get("id") {
                    let sub_id = sub_id.as_s().unwrap();
                    sub_ids.push(sub_id.to_string());
                }
            }
        }

        self.delete_subscriptions(sub_ids).await
    }

    pub async fn get_all_subscriptions(&self) -> Vec<(String, String, Vec<Filter>)> {
        let table = std::env::var("NOSTR_SUBSCRIPTION_TABLE").unwrap();
        let mut results = vec![];

        let items: Result<Vec<_>, _> = self
            .client
            .scan()
            .table_name(table)
            .into_paginator()
            .items()
            .send()
            .collect()
            .await;

        if let Ok(items) = items {
            for item in items {
                let sub_id = if let Some(sub_id) = item.get("id") {
                    let sub_id = sub_id.as_s().unwrap();
                    sub_id.to_string()
                } else {
                    break;
                };
                let conn_id = if let Some(conn_id) = item.get("value") {
                    conn_id.as_s().unwrap().clone()
                } else {
                    break;
                };
                let filters = if let Some(fs) = item.get("filters") {
                    let rfs = fs.as_l().unwrap();
                    let vs: Vec<String> =
                        rfs.iter().map(|f| f.as_s().unwrap().to_string()).collect();
                    vs
                } else {
                    break;
                };
                let filters = filters
                    .iter()
                    .map(|f| serde_json::from_str(f).unwrap())
                    .collect();
                results.push((sub_id, conn_id, filters));
            }
        }

        results
    }

    pub async fn get_event_by_ids(&self, ids: &[String]) -> Result<Vec<Event>, String> {
        let table = std::env::var("NOSTR_EVENT_TABLE").unwrap();

        let keys = ids
            .iter()
            .fold(KeysAndAttributes::builder(), |builder, id| {
                builder.keys(HashMap::from([
                    ("id".to_string(), AttributeValue::S(id.to_string())),
                    ("type".to_string(), AttributeValue::S("event".to_string())),
                ]))
            })
            .build();

        let items = self
            .client
            .batch_get_item()
            .request_items(&table, keys)
            .send()
            .await;

        match items {
            Err(e) => Err(format!("{e:?}")),
            Ok(item) => {
                if let Some(ret) = item.responses() {
                    let v = ret.get(&table).unwrap();
                    let vv: Vec<&AttributeValue> =
                        v.iter().map(|hm| hm.get("json").unwrap()).collect();
                    let vvv: Vec<String> =
                        vv.iter().map(|a| a.as_s().unwrap().to_string()).collect();
                    let vvvv = vvv
                        .iter()
                        .map(|json| serde_json::from_str(json).unwrap())
                        .collect();
                    Ok(vvvv)
                } else {
                    Err("none".to_string())
                }
            }
        }
    }

    pub async fn get_event_by_pubkeys(
        &self,
        pubkeys: &[String],
        kinds: Option<Vec<u64>>,
        since: Option<u64>,
        until: Option<u64>,
        limit: Option<i32>,
    ) -> Result<Vec<Event>, String> {
        let since = since.unwrap_or(0);
        let until = until.unwrap_or(1893456000);
        let mut count = limit.unwrap_or(100);
        let mut result = vec![];

        for pubkey in pubkeys {
            if let Ok(evs) = self
                .get_event_by_pubkey(pubkey, &kinds, since, until, count)
                .await
            {
                count -= evs.len() as i32;
                result.extend(evs);
            }
            if count <= 0 {
                break;
            }
        }

        Ok(result)
    }

    async fn get_event_by_pubkey(
        &self,
        pubkey: &str,
        kinds: &Option<Vec<u64>>,
        since: u64,
        until: u64,
        limit: i32,
    ) -> Result<Vec<Event>, String> {
        let table = std::env::var("NOSTR_EVENT_TABLE").unwrap();

        let query = self
            .client
            .query()
            .limit(limit)
            .table_name(table)
            .index_name("pubkey-created_at-index")
            .key_condition_expression("pubkey = :pubkey AND (created_at BETWEEN :since AND :until)")
            .expression_attribute_values(":pubkey", AttributeValue::S(pubkey.to_string()))
            .expression_attribute_values(":since", AttributeValue::N(since.to_string()))
            .expression_attribute_values(":until", AttributeValue::N(until.to_string()));

        let query = if let Some(kinds) = kinds {
            let mut keys = vec![];
            let mut vals = vec![];
            for (i, kind) in kinds.iter().enumerate() {
                keys.push(format!(":kind{i}"));
                vals.push((format!(":kind{i}"), AttributeValue::N(kind.to_string())));
            }
            let kind_labels = keys.join(",");
            vals.iter().fold(
                query.filter_expression(format!("kind IN({kind_labels})")),
                |builder, (label, value)| builder.expression_attribute_values(label, value.clone()),
            )
        } else {
            query
        };

        let items: Result<Vec<_>, _> = query
            .into_paginator()
            .items()
            .send()
            .take(limit as usize)
            .collect()
            .await;
        let mut ids = vec![];
        if let Ok(items) = items {
            for item in items {
                if let Some(id) = item.get("id") {
                    ids.push(id.as_s().unwrap().to_string())
                }
            }
        }
        self.get_event_by_ids(&ids).await
    }

    pub async fn delete_event_by_ids(
        &self,
        ids: Vec<String>,
    ) -> Result<
        aws_sdk_dynamodb::output::BatchWriteItemOutput,
        aws_sdk_dynamodb::types::SdkError<aws_sdk_dynamodb::error::BatchWriteItemError>,
    > {
        let table = std::env::var("NOSTR_EVENT_TABLE").unwrap();
        let mut wrs = Vec::<WriteRequest>::new();

        for id in ids {
            wrs.push(delete_request(&id, "event"));
        }

        self.client
            .batch_write_item()
            .request_items(table, wrs)
            .send()
            .await
    }
}

fn write_request(
    id: &str,
    item_type: &str,
    value: AttributeValue,
    data: Option<Vec<(String, AttributeValue)>>,
    ttl: i64,
) -> WriteRequest {
    let mut map = HashMap::new();
    map.insert("id".to_string(), AttributeValue::S(id.to_string()));
    map.insert("type".to_string(), AttributeValue::S(item_type.to_string()));
    map.insert("value".to_string(), value);

    if let Some(vs) = data {
        for (k, v) in vs {
            map.insert(k.to_string(), v);
        }
    }

    if ttl >= 0 {
        map.insert("_ttl".to_string(), AttributeValue::N(ttl.to_string()));
    }

    let pr = PutRequest::builder().set_item(Some(map)).build();

    WriteRequest::builder().put_request(pr).build()
}

fn delete_request(id: &str, item_type: &str) -> WriteRequest {
    let mut map = HashMap::new();
    map.insert("id".to_string(), AttributeValue::S(id.to_string()));
    map.insert("type".to_string(), AttributeValue::S(item_type.to_string()));

    let dr = DeleteRequest::builder().set_key(Some(map)).build();

    WriteRequest::builder().delete_request(dr).build()
}

pub struct QueryByIds<'a> {
    filter: &'a Filter,
    ids: Vec<String>,
}

impl<'a> QueryByIds<'a> {
    pub fn new(filter: &'a Filter, ids: Vec<String>) -> QueryByIds<'a> {
        QueryByIds { filter, ids }
    }

    pub async fn exec(&self) -> Result<Vec<Event>, String> {
        let ddb = Ddb::new().await;
        let ret = ddb.get_event_by_ids(&self.ids).await;

        filter_match(self.filter, &ret)
    }
}

fn filter_match(filter: &Filter, evs: &Result<Vec<Event>, String>) -> Result<Vec<Event>, String> {
    match evs {
        Ok(ret) => {
            let vmatch = ret
                .iter()
                .filter_map(|e| {
                    if filter.event_match(e) {
                        Some(e.clone())
                    } else {
                        None
                    }
                })
                .collect();
            Ok(vmatch)
        }
        Err(e) => Err(e.to_string()),
    }
}

pub struct QueryByPubkeys<'a> {
    filter: &'a Filter,
    authors: Vec<String>,
    kinds: Option<Vec<u64>>,
    since: Option<u64>,
    until: Option<u64>,
    limit: Option<i32>,
}

impl<'a> QueryByPubkeys<'a> {
    pub fn new(
        filter: &'a Filter,
        authors: Vec<String>,
        kinds: Option<Vec<u64>>,
        since: Option<u64>,
        until: Option<u64>,
        limit: Option<i32>,
    ) -> QueryByPubkeys {
        QueryByPubkeys {
            filter,
            authors,
            kinds,
            since,
            until,
            limit,
        }
    }

    pub async fn exec(&self) -> Result<Vec<Event>, String> {
        let ddb = Ddb::new().await;
        let ret = ddb
            .get_event_by_pubkeys(
                &self.authors,
                self.kinds.clone(),
                self.since,
                self.until,
                self.limit,
            )
            .await;

        filter_match(self.filter, &ret)
    }
}

pub enum QueryPlan<'a> {
    ByIds(QueryByIds<'a>),
    ByPubkeys(QueryByPubkeys<'a>),
    NoPlan(String),
}
