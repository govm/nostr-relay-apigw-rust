use lambda_http::request::RequestContext;
use lambda_http::{run, service_fn, Body, Error, Request, RequestExt, Response};
use nostr_relay_apigw::{message, relay};

fn build_messagectx(request: &Request) -> message::MessageContext {
    let ctx = if let RequestContext::WebSocket(ctx) = request.request_context() {
        ctx
    } else {
        panic!("expect websocket");
    };
    message::MessageContext::new(
        &ctx.connection_id.unwrap(),
        &format!(
            "https://{}/{}",
            ctx.domain_name.unwrap(),
            ctx.stage.unwrap()
        ),
        &ctx.route_key.unwrap(),
        ctx.request_time_epoch.try_into().unwrap(),
    )
}

fn parse_eventmsg(message: &str) -> Option<message::EventCmd> {
    let ret = serde_json::from_str(message);
    if let Err(err) = ret {
        println!("err: {err}");
        return None;
    }
    let arr: Vec<message::EventMsg> = ret.unwrap();
    if let (message::EventMsg::String(cmd), message::EventMsg::Event(ev)) = (&arr[0], &arr[1]) {
        Some(message::EventCmd::new(cmd, ev))
    } else {
        None
    }
}

fn parse_reqmsg(message: &str) -> Option<message::ReqCmd> {
    let ret = serde_json::from_str(message);
    if let Err(err) = ret {
        println!("err: {err}");
        return None;
    }
    let arr: Vec<message::ReqMsg> = ret.unwrap();
    let cmd = if let message::ReqMsg::String(cmd) = &arr[0] {
        cmd
    } else {
        return None;
    };
    let sub_id = if let message::ReqMsg::String(sub_id) = &arr[1] {
        sub_id
    } else {
        return None;
    };
    let mut fs = vec![];
    for v in arr[2..].iter() {
        if let message::ReqMsg::Filter(fl) = v {
            fs.push(fl.clone())
        }
    }

    Some(message::ReqCmd::new(cmd, sub_id, fs))
}

fn parse_closemsg(message: &str) -> Option<message::CloseCmd> {
    let ret = serde_json::from_str(message);
    if let Err(err) = ret {
        println!("err: {err}");
        return None;
    }
    let arr: Vec<message::CloseMsg> = ret.unwrap();
    let message::CloseMsg::String(cmd) = &arr[0];
    let message::CloseMsg::String(sub_id) = &arr[1];

    Some(message::CloseCmd::new(cmd, sub_id))
}

async fn function_handler_http(_event: Request) -> Result<Response<Body>, Error> {
    let resp = Response::builder()
        .status(200)
        .header("content-type", "application/nostr+json")
        .body(nostr_relay_apigw::nip11::json().into())
        .map_err(Box::new)?;
    Ok(resp)
}

/// This is the main body for the function.
/// Write your code inside it.
/// There are some code example in the following URLs:
/// - https://github.com/awslabs/aws-lambda-rust-runtime/tree/main/examples
async fn function_handler(event: Request) -> Result<Response<Body>, Error> {
    // Extract some useful information from the request

    println!("event: {event:?}");
    if let lambda_http::request::RequestContext::WebSocket(ctx) = event.request_context() {
        println!("context: {ctx:?}");
    } else {
        return function_handler_http(event).await;
    }

    let ctx = build_messagectx(&event);
    if !event.body().is_empty() {
        if let Body::Text(msg) = event.body() {
            match &*ctx.command {
                "EVENT" => relay::process_event(&ctx, &parse_eventmsg(msg)).await,
                "REQ" => relay::process_req(&ctx, &parse_reqmsg(msg)).await,
                "CLOSE" => relay::process_close(&ctx, &parse_closemsg(msg)).await,
                c => println!("default: command: {c}"),
            }
        }
    } else {
        match &*ctx.command {
            "$disconnect" => relay::process_disconn(&ctx).await,
            c => println!("default: command: {c}"),
        }
    }

    // Return something that implements IntoResponse.
    // It will be serialized to the right response event automatically by the runtime
    let resp = Response::builder()
        .status(200)
        .header("content-type", "text/html")
        .body("Hello AWS Lambda HTTP request".into())
        .map_err(Box::new)?;
    Ok(resp)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        // disable printing the name of the module in every log line.
        .with_target(false)
        // disabling time is handy because CloudWatch will add the ingestion time.
        .without_time()
        .init();

    run(service_fn(function_handler)).await
}

#[cfg(test)]
mod tests {
    use super::parse_closemsg;
    use super::parse_eventmsg;
    use super::parse_reqmsg;
    use serde_json;

    #[test]
    fn parse_reqmsg01() {
        let msg = r#"["REQ", "sub_id01", {"authors": ["npub1xxx"]}]"#;
        let ret = parse_reqmsg(msg).expect("REQ");
        assert_eq!(
            r#"{"cmd":"REQ","subscription_id":"sub_id01","filters":[{"authors":["npub1xxx"]}]}"#,
            serde_json::to_string(&ret).unwrap()
        );
    }

    #[test]
    fn parse_eventmsg01() {
        let msg = r#"["EVENT", {"id": "id01", "pubkey": "npub1yyy", "created_at": 1675949672, "kind": 0,
                            "tags":[["e", "0000"], ["p", "1111"]],
                            "content": "content",
                            "sig": "sig01"}]"#;
        let ret = parse_eventmsg(msg).expect("EVENT");
        assert_eq!(
            r#"{"cmd":"EVENT","event":{"id":"id01","pubkey":"npub1yyy","created_at":1675949672,"kind":0,"tags":[["e","0000"],["p","1111"]],"content":"content","sig":"sig01"}}"#,
            serde_json::to_string(&ret).unwrap()
        );
    }

    #[test]
    fn parse_closemsg01() {
        let msg = r#"["CLOSE", "sub_id01"]"#;
        let ret = parse_closemsg(msg).expect("CLOSE");
        assert_eq!(
            r#"{"cmd":"CLOSE","subscription_id":"sub_id01"}"#,
            serde_json::to_string(&ret).unwrap()
        );
    }
}
