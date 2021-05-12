use crate::commands::{
    check_msg,
    MusicQueue,
    CurrentSong,
    make_source,
};
use std::{
    time::{
        Instant,
    },
    collections::{
        VecDeque,
    },
};
use serenity::{
    framework::standard::{
        CommandResult,
        Args,
        macros::{
            command,
        },
    },
    client::Context,
    model::{
        channel::Message,
    },
};


async fn _skip(ctx: &Context, msg: &Message) -> CommandResult {
    // Discord uses the name guild but it's the server
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird voice client was not initialized at serenity start up");

    // get the handler for the voice channel we're a part of, if we're not in a voice channel then
    // we just keep trucking, this isn't the command to put the bot in a voice chat
    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        let mut data = ctx.data.write().await;
        let mut song_map = data.get_mut::<MusicQueue>().expect("Expected a song queue set up in the main.rs file").lock().await;
        let queue = song_map.entry(guild_id.clone()).or_insert(VecDeque::new());
        let pos_song = queue.pop_front();
        drop(song_map);
        if let Some(song) = pos_song {
            handler.play_only_source(make_source(&song).unwrap());
            check_msg(msg.channel_id.say(&ctx.http, &format!("Playing {}", song.json_map.get("title").and_then(serde_json::Value::as_str).unwrap())).await);
            let moment = Instant::now();
            let mut cur_map = data.get_mut::<CurrentSong>().expect("Expected a CurrentSong object set up in the main.rs file").lock().await;
            let (pos_ins, cur_song) = cur_map.entry(guild_id.clone()).or_insert((None, song.clone()));
            *pos_ins = Some(moment);
            *cur_song = song.clone();
        }
    };
    Ok(())
}


#[command]
#[only_in(guilds)]
async fn skip(ctx: &Context, msg: &Message) -> CommandResult {
    _skip(ctx,msg).await
}

#[command]
#[only_in(guilds)]
async fn skipto(ctx: &Context, msg:&Message, mut args: Args) -> CommandResult {
    let number = match args.single::<usize>(){
        Ok(url) => url,
        Err(_) => {
            check_msg(msg.channel_id.say(&ctx.http, "You need a url after the command, doofus").await);
            return Ok(());
        },
    };
    // Discord uses the name guild but it's the server
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird voice client was not initialized at serenity start up");

    // get the handler for the voice channel we're a part of, if we're not in a voice channel then
    // we just keep trucking, this isn't the command to put the bot in a voice chat
    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        let mut data = ctx.data.write().await;
        let mut song_map = data.get_mut::<MusicQueue>().expect("Expected a song queue set up in the main.rs file").lock().await;
        let queue = song_map.entry(guild_id.clone()).or_insert(VecDeque::new());
        if number-1 > queue.len(){
            check_msg(msg.reply(&ctx.http, "There's not enough songs in the queue").await);
            return Ok(());
        }
        *queue = queue.split_off(number-1);
        let pos_song = queue.pop_front();
        drop(song_map);
        if let Some(song) = pos_song {
            handler.play_only_source(make_source(&song).unwrap());
            check_msg(msg.channel_id.say(&ctx.http, &format!("Playing {}", song.json_map.get("title").and_then(serde_json::Value::as_str).unwrap())).await);
            let moment = Instant::now();
            let mut cur_map = data.get_mut::<CurrentSong>().expect("Expected a CurrentSong object set up in the main.rs file").lock().await;
            let (pos_ins, cur_song) = cur_map.entry(guild_id.clone()).or_insert((None, song.clone()));
            *pos_ins = Some(moment);
            *cur_song = song.clone();
        }
    };

    Ok(())
}
