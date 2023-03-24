use ambient_api::{
    message::server::{MessageExt, Source, Target},
    prelude::*,
};

#[main]
pub async fn main() -> EventResult {
    messages::Hello::subscribe(|source, data| {
        let Source::Network { user_id } = source else { return EventOk; };
        println!("{user_id}: {:?}", data);

        let source_reliable = data.source_reliable;

        messages::Hello::new(
            true,
            format!("{source_reliable}: Hello, world from the server!"),
        )
        .send(Target::NetworkTargetedReliable(user_id.clone()));

        messages::Hello::new(
            false,
            format!("{source_reliable}: Hello, world from the server!"),
        )
        .send(Target::NetworkTargetedUnreliable(user_id.clone()));

        messages::Hello::new(
            true,
            format!("{source_reliable}: Hello, world (everyone) from the server!"),
        )
        .send(Target::NetworkBroadcastReliable);

        EventOk
    });

    let handled = State::new(false);
    messages::Local::subscribe({
        let handled = handled.clone();
        move |source, data| {
            println!("{source:?}: {data:?}");
            *handled.write() = true;

            EventOk
        }
    });
    run_async(async move {
        loop {
            if *handled.read() {
                break;
            }

            sleep(1.0).await;
            messages::Local::new("Hello!").send(Target::ModuleBroadcast);
        }
        EventOk
    });

    EventOk
}
