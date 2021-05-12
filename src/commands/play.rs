use tracing::{error, info};
use crate::commands::{
    check_msg,
    MusicQueue,
    CurrentSong,
    make_source,
    pull_youtube_child,
    process_output,
};
use std::{
    io::{
        BufRead,
        BufReader,
    },
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

#[command]
#[only_in(guilds)]
async fn play(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    if args.is_empty(){
        resume(ctx, msg).await
    }else{
        _play(ctx, msg, args).await
    }
}

#[command]
#[only_in(guilds)]
async fn mechanicus(ctx: &Context, msg:&Message) -> CommandResult {
    check_msg(msg.reply(&ctx.http, "As the Omnissiah wills").await);
    _play(ctx, msg, Args::new("https://www.youtube.com/watch?v=9gIMZ0WyY88", &[])).await
}

async fn resume(ctx: &Context, msg: &Message) -> CommandResult {
    // Discord uses the name guild but it's the server
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states.get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let manager = songbird::get(ctx).await
        .expect("Songbird voice client was not initialized at serenity start up");

    // get the voice channel ID
    let connect_to = match channel_id{
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "You need to be in a voice channel").await);

            return Ok(());
        },
    };
    // get the handler for the voice channel we're a part of, if we're not in a voice channel then
    // we try to join the one the author of message is a part of
    let handler_lock = match manager.get(guild_id){
        Some(handler) => handler,
        None => {
            let res = manager.join(guild_id, connect_to).await;
            match res.1{
                Ok(_) => {
                    res.0
                },
                Err(err) => {
                    error!("Failed to join voice channel: {:?}", err);
                    check_msg(msg.channel_id.say(&ctx.http, "Unable to join voice channel, go yell at Brandon").await);
                    return Ok(());
                },
            }
        },
    };

    let mut data = ctx.data.write().await;
    // get the current song queue
    let pos_cur_map = data.get_mut::<CurrentSong>().expect("Expected a current song object");
    let mut cur_map = pos_cur_map.lock().await;
    // the Tuple is (Option<Instant>, SongInfo)
    if let Some((pos_ins, song)) = cur_map.get_mut(&guild_id){
        let mut handler = handler_lock.lock().await;
        handler.play_only_source(make_source(&song).unwrap());
        let moment = Instant::now();

        *pos_ins = Some(moment);
    }else{
        check_msg(msg.channel_id.say(&ctx.http, "There's nothing in the queue").await);
    };
    Ok(())
}

async fn _play(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let url = match args.single::<String>(){
        Ok(url) => url,
        Err(_) => {
            check_msg(msg.channel_id.say(&ctx.http, "You need a url after the command, doofus").await);
            return Ok(());
        },
    };
    // Discord uses the name guild but it's the server
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states.get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    // get the voice channel ID
    let connect_to = match channel_id{
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "You need to be in a voice channel").await);

            return Ok(());
        },
    };

    let manager = songbird::get(ctx).await
        .expect("Songbird voice client was not initialized at serenity start up").clone();

    // get the handler for the voice channel we're a part of, if we're not in a voice channel then
    // we try to join the one the author of message is a part of
    let handler_lock = match manager.get(guild_id){
        Some(handler) => handler,
        None => {
            let res = manager.join(guild_id, connect_to).await;
            match res.1{
                Ok(_) => {
                    res.0
                },
                Err(err) => {
                    error!("Failed to join voice channel: {:?}", err);
                    check_msg(msg.channel_id.say(&ctx.http, "Unable to join voice channel, go yell at Brandon").await);
                    return Ok(());
                },
            }
        },
    };

    let mut handler = handler_lock.lock().await;
    if !handler.is_deaf(){
        if let Err(err) = handler.deafen(true).await {
            error!("Deafen failed: {:?}", err);
        };
    }

    let mut comm = pull_youtube_child(url);
    
    // outputs the stdout to a buffer so we can read it later
    let mut comm_out = BufReader::new(comm.stdout.as_mut().unwrap());
    let mut dat = String::new();
    comm_out.read_line(&mut dat).unwrap();
    // process the json text into a usable SongInfo object
    let cur_song = match process_output(dat.clone(), msg.channel_id){
        Some(song) => song,
        None => {
            check_msg(msg.channel_id.say(&ctx.http, "Failed to process the link, perhaps it was an unsupported link").await);
            return Ok(());
        },
    };


    check_msg(msg.channel_id.say(&ctx.http, &format!("Playing {}", cur_song.json_map.get("title").and_then(serde_json::Value::as_str).unwrap())).await);
    // Make it into a source so the handler can actually play it
    handler.play_only_source(make_source(&cur_song).unwrap());
    let moment = Instant::now();

    // get a writable version of the custom data in the context
    let mut data = ctx.data.write().await;

    // get the current song queue
    let pos_cur_map = data.get_mut::<CurrentSong>().expect("Expected a current song object");
    let mut cur_map = pos_cur_map.lock().await;
    // the Tuple is (Option<Instant>, SongInfo)
    let (pos_ins, song) = cur_map.entry(guild_id.clone()).or_insert((None, cur_song.clone()));
    *pos_ins = Some(moment);
    *song = cur_song.clone();
    // drop it so we can get the queue
    drop(cur_map);
    let mut song_map = data.get_mut::<MusicQueue>().expect("Expected a song queue").lock().await;
    // If there's no queue that exists we'll add an empty one
    let queue = song_map.entry(guild_id.clone()).or_insert(VecDeque::new());
    loop{
        // Clear the data, read_line appends to the string
        dat = "".to_owned();
        // it returns the amount of bytes read from the buffer, if it's zero than we're done
        if comm_out.read_line(&mut dat).expect("This shouldn't fail") == 0{
            break;
        }
        // Make it into a SongInfo object and add it to the queue
        match process_output(dat.clone(), msg.channel_id){
            Some(song) => {
                info!("Queued song {}", song.clone().json_map.get("title").and_then(serde_json::Value::as_str).unwrap());
                queue.push_back(song);
            },
            None => {
                error!("There was a problem proccessing the json for a video");
                check_msg(msg.channel_id.say(&ctx.http, "There was a problem processing a video in the playlist, it was not added").await);
            },
        };
    }
    check_msg(msg.channel_id.say(&ctx.http, &format!("{} songs are in the queue", queue.len())).await);
    drop(song_map);

    Ok(())
}
