use tracing::{error, info};
use crate::commands::{
    check_msg,
    SongInfo,
    MusicQueue,
    CurrentSong,
};
use std::{
    process::{
        Command,
        Stdio,
        Child,
    },
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
        id::ChannelId,
        channel::Message,
    },
};
use songbird::{
    input::{
        Codec,
        Container,
        Metadata,
        Input,
        child::{
            children_to_reader,
        },
    },
};

use serde_json::{
    Result as JsonResult,
    Value,
};

pub fn pull_youtube_child(url: String) -> Child{
    Command::new("youtube-dl")
        .args(&[
            "-f",
            "webm[abr>0]/bestaudio/best",
            "--print-json",
            "--skip-download",
            "-u",
            "brandon.haley94@gmail.com",
            "-p",
            "H6A3l5e5yb1!!",
            //"--newline",
            &url,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
}

pub fn process_output(data: String, chan: ChannelId) -> Option<SongInfo>{
    // The input should be a raw json object in text
    let res: JsonResult<Value> = serde_json::from_str(&data);
    let json_map = match res{
        Ok(v) => {
            // return it as a map with the signature SerdeMap<String, Value>
            v.as_object()?.clone()
        },
        Err(err) => {
            // Shit's gone sideways
            error!("Error parsing youtube json: {:?}", err);
            return None
        },
    };
    // Slap that json_map in there, it's got all the info we could ever want on the song
    // I've added the channel so that everyone knows where to send messages too 
    Some(SongInfo{
        json_map: json_map,
        channel: chan,
    })
}

pub fn make_source(data: &SongInfo) -> Option<Input>{
    // This actually runs in the background and feeds data to the websocket, that's pretty cool
    let ffmpeg = Command::new("ffmpeg")
        .arg("-i")
        .arg(data.json_map.get("url").and_then(serde_json::Value::as_str)?) //this is the only point of failure
        .args(&[
            "-loglevel",
            "quiet",
            "-hide_banner",
            "-f",
            "s16le",// THIS IS AN L NOT A 1, THIS FUCKING FONT 
            "-ac", 
            "2",
            "-ar",
            "48000",
            "-acodec",
            "pcm_f32le", // this if f32 little edian because that's what songbird needs it to be
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let metadata = Metadata::from_ytdl_output(Value::Object(data.json_map.clone()));

    Some(Input::new(
            true, // It's stereo
            children_to_reader::<f32>(vec![ffmpeg]), // This is the actual data from the ffmpeg program running in the background
            Codec::FloatPcm, //this is the codec we put in the up above
            Container::Raw, // IT'S FOOKIN RAW
            Some(metadata), // metadata taken from the youtube json object
    ))
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
async fn skip(ctx: &Context, msg: &Message) -> CommandResult {
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
async fn add(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
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


    // get the handler for the voice channel we're a part of, if we're not in a voice channel then
    // we just keep trucking, this isn't the command to put the bot in a voice chat
    let mut comm = pull_youtube_child(url);
    
    // outputs the stdout to a buffer so we can read it later
    let mut comm_out = BufReader::new(comm.stdout.as_mut().unwrap());
    let mut data = ctx.data.write().await;
    let mut song_map = data.get_mut::<MusicQueue>().expect("Expected a song queue set up in the main.rs file").lock().await;
    let queue = song_map.entry(guild_id.clone()).or_insert(VecDeque::new());

    loop{
        // Clear the data, read_line appends to the string
        let mut dat = String::new();
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


    Ok(())
}

#[command]
#[only_in(guilds)]
async fn pause(ctx: &Context, msg:&Message) -> CommandResult {
    // Discord uses the name guild but it's the server
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird voice client was not initialized at serenity start up");

    // get the handler for the voice channel we're a part of, if we're not in a voice channel then
    // we just keep trucking, this isn't the command to put the bot in a voice chat
    if let Some(handler_lock) = manager.get(guild_id) {
        let mut data = ctx.data.write().await;
        let mut cur_map = data.get_mut::<CurrentSong>().expect("Expected a CurrentSong object set up in the main.rs file").lock().await;
        if let Some((pos_ins, cur_song)) = cur_map.get_mut(&guild_id){
            let mut handler = handler_lock.lock().await;
            handler.stop();
            check_msg(msg.channel_id.say(&ctx.http, &format!("Pausing {}", cur_song.json_map.get("title").and_then(serde_json::Value::as_str).unwrap())).await);
            *pos_ins = None;
        }
    };
    Ok(())
}

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

#[command]
#[only_in(guilds)]
async fn mechanicus(ctx: &Context, msg:&Message) -> CommandResult {
    check_msg(msg.reply(&ctx.http, "As the Omnissiah wills").await);
    _play(ctx, msg, Args::new("https://www.youtube.com/watch?v=9gIMZ0WyY88", &[])).await
}
