use crate::commands::{
    check_msg,
    MusicQueue,
};
use serenity::{
    framework::standard::{
        CommandResult,
        macros::{
            command,
        },
    },
    client::Context,
    model::{
        channel::Message,
    },
};


#[command]
#[only_in(guilds)]
async fn queue(ctx: &Context, msg:&Message) -> CommandResult {
    // Discord uses the name guild but it's the server
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let data = ctx.data.write().await;
    let queue_map = data.get::<MusicQueue>().expect("Expected a song queue set up in the main.rs file").lock().await;
    if let Some(queue) = queue_map.get(&guild_id){
        check_msg(msg.channel_id.send_message(&ctx.http, |m| {
            m.embed(|e| {
                e.title("Music Queue");
                for (num, song) in queue.iter().enumerate(){
                    e.field(num+1, song.json_map.get("title").and_then(serde_json::Value::as_str).unwrap(), true);
                }
                e
            });

            m
        }).await);
    }else{
        check_msg(msg.channel_id.say(&ctx.http, "The queue is empty").await);
    }

    Ok(())
}

