use crate::commands::{
    check_msg,
    MusicQueue,
    CurrentSong,
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
async fn stop(ctx: &Context, msg:&Message) -> CommandResult {
    // Discord uses the name guild but it's the server
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird voice client was not initialized at serenity start up");

    // get the handler for the voice channel we're a part of, if we're not in a voice channel then
    // we just keep trucking, this isn't the command to put the bot in a voice chat
    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        handler.stop();
        handler.leave().await.unwrap();
        check_msg(msg.reply(ctx, "See you space cowboy").await);
        let mut data = ctx.data.write().await;
        let mut cur_map = data.get_mut::<CurrentSong>().expect("Expected a CurrentSong object set up in the main.rs file").lock().await;
        cur_map.remove(&guild_id);
        drop(cur_map);
        let mut queue_map = data.get_mut::<MusicQueue>().expect("Expected a song queue set up in the main.rs file").lock().await;
        if let Some(queue) = queue_map.get_mut(&guild_id){
            queue.clear();
        }
        check_msg(msg.channel_id.say(&ctx.http, "The queue has been purged of filth").await);
    };
    Ok(())
}
