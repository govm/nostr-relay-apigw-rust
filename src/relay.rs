use crate::apigwmgmt::ApiGwMgmt;
use crate::ddb::Ddb;
use crate::ddb::QueryPlan;
use crate::hook::HOOKS;
use crate::message::{CloseCmd, Event, EventCmd, MessageContext, ReqCmd};
use std::collections::HashSet;

pub async fn process_event(ctx: &MessageContext, cmd: &Option<EventCmd>) {
    if let Some(cmd) = cmd {
        println!(
            "cmd: {}, conn: {}, event: {:?}",
            cmd.cmd, ctx.connection_id, cmd.event
        );
        let api = ApiGwMgmt::new(&ctx.endpoint).await;
        if cmd.event.pubkey != "14e83f2cffa739fa7d88de86acfe8edf0750841c9460ebf7e1c56ff381d89666"
            && cmd.event.pubkey
                != "98f4285bcb2cc65c3a66bd77ccffd2563ed3303e7e02a489c63a887fcd06bbe5"
        {
            api.send_nip20msg(
                &ctx.connection_id,
                &cmd.event.id,
                false,
                "blocked: not allowed",
            )
            .await;
            return;
        }
        if let Err(reason) = cmd.event.validate() {
            println!("sig:{reason}");
            api.send_nip20msg(
                &ctx.connection_id,
                &cmd.event.id,
                false,
                "invalid: signature is wrong",
            )
            .await;
        } else {
            println!("sig:ok");
            let ddb = Ddb::new().await;
            HOOKS.pre_event_write_hook(&cmd.event).await;
            write_event(&ddb, ctx, &cmd.event).await;
            HOOKS.post_event_write_hook(&cmd.event).await;
            dispatch_event(&ddb, ctx, &cmd.event).await;
        }
    }
}

async fn write_event(ddb: &Ddb, ctx: &MessageContext, event: &Event) {
    let api = ApiGwMgmt::new(&ctx.endpoint).await;

    if event.is_nip16_ephemeral() {
        api.send_nip20msg(&ctx.connection_id, &event.id, true, "")
            .await;
        return;
    }

    let ret = ddb.write_event(event).await;
    match ret {
        Ok(r) => {
            println!("ddb ok: {r:?}");
            api.send_nip20msg(&ctx.connection_id, &event.id, true, "")
                .await;
        }
        Err(r) => {
            println!("ddb err: {r:?}");
            api.send_nip20msg(
                &ctx.connection_id,
                &event.id,
                false,
                "error: failed to save the event",
            )
            .await;
        }
    }
}

async fn dispatch_event(ddb: &Ddb, ctx: &MessageContext, event: &Event) {
    let api = ApiGwMgmt::new(&ctx.endpoint).await;
    let v = ddb.get_all_subscriptions().await;
    for (sub, conn, fs) in v {
        for f in fs {
            if f.event_match(event) {
                api.reply_event(&sub, &conn, event).await;
            }
        }
    }
}

pub async fn process_req(ctx: &MessageContext, cmd: &Option<ReqCmd>) {
    if let Some(cmd) = cmd {
        println!(
            "cmd: {}, conn: {}, arg: {:?}",
            cmd.cmd, ctx.connection_id, cmd
        );

        let ddb = crate::ddb::Ddb::new().await;
        let ret = ddb
            .write_subscription(&ctx.connection_id, &cmd.subscription_id, &cmd.filters)
            .await;
        match ret {
            Ok(r) => {
                println!("ddb ok: {r:?}");
                let api = ApiGwMgmt::new(&ctx.endpoint).await;
                let mut evs: Vec<Event> = vec![];
                for f in &cmd.filters {
                    let r = match f.query_plan() {
                        QueryPlan::ByIds(plan) => plan.exec().await,
                        QueryPlan::ByPubkeys(plan) => plan.exec().await,
                        _ => {
                            api.send_nip15eose(&ctx.connection_id, &cmd.subscription_id)
                                .await;
                            return;
                        }
                    };
                    if let Ok(r) = r {
                        evs.extend(r);
                    }
                }
                let evsh: HashSet<&Event> = evs.iter().collect();

                for ev in evsh {
                    api.reply_event(&cmd.subscription_id, &ctx.connection_id, ev)
                        .await;
                }
                api.send_nip15eose(&ctx.connection_id, &cmd.subscription_id)
                    .await;
            }
            Err(r) => println!("ddb err: {r:?}"),
        }
    }
}

pub async fn process_close(ctx: &MessageContext, cmd: &Option<CloseCmd>) {
    if let Some(cmd) = cmd {
        println!(
            "cmd: {}, conn: {}, sub_id: {}",
            cmd.cmd, ctx.connection_id, cmd.subscription_id
        );

        let ddb = crate::ddb::Ddb::new().await;
        let ret = ddb
            .delete_subscriptions(vec![cmd.subscription_id.to_string()])
            .await;
        match ret {
            Ok(r) => println!("ddb ok: {r:?}"),
            Err(r) => println!("ddb err: {r:?}"),
        }
    }
}

pub async fn process_disconn(ctx: &MessageContext) {
    println!("cmd: {}, conn: {}", ctx.command, ctx.connection_id);

    let ddb = crate::ddb::Ddb::new().await;
    let _ret = ddb.close_connection(&ctx.connection_id).await;
}
