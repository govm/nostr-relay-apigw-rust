use crate::message::{CommandResult, Event, EventMsg};
use aws_sdk_apigatewaymanagement::types::Blob;
use aws_sdk_apigatewaymanagement::{config, Client};

pub struct ApiGwMgmt {
    client: Client,
}

impl ApiGwMgmt {
    pub async fn new(endpoint: &str) -> ApiGwMgmt {
        let shared_config = aws_config::load_from_env().await;
        let config = config::Builder::from(&shared_config)
            .endpoint_url(endpoint)
            .build();
        let client = Client::from_conf(config);

        ApiGwMgmt { client }
    }

    pub async fn post_connection(&self, conn_id: &str, data: &str) -> bool {
        let result = self
            .client
            .post_to_connection()
            .connection_id(conn_id)
            .data(Blob::new(data))
            .send()
            .await;

        if let Err(e) = result {
            println!("post_connection err: {e:?}");
            false
        } else {
            true
        }
    }

    pub async fn reply_event(&self, sub: &str, conn: &str, ev: &Event) -> bool {
        let obj = [
            EventMsg::String("EVENT".to_string()),
            EventMsg::String(sub.to_string()),
            EventMsg::Event(ev.clone()),
        ];
        let msg = serde_json::to_string(&obj).unwrap();
        println!("reply_event: {sub}/{conn}: {msg}");
        self.post_connection(conn, &msg).await
    }

    pub async fn send_nip20msg(
        &self,
        conn: &str,
        event_id: &str,
        success: bool,
        msg: &str,
    ) -> bool {
        let obj = [
            CommandResult::String("OK".to_string()),
            CommandResult::String(event_id.to_string()),
            CommandResult::Bool(success),
            CommandResult::String(msg.to_string()),
        ];
        let msg = serde_json::to_string(&obj).unwrap();
        self.post_connection(conn, &msg).await
    }

    pub async fn send_nip15eose(&self, conn: &str, sub_id: &str) -> bool {
        let msg = format!(r#"["EOSE", "{sub_id}"]"#);
        self.post_connection(conn, &msg).await
    }
}
