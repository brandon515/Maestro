use std::{
    process::{
        Command,
        Stdio,
        Child,
    },
    time::{
        Instant,
    },
    collections::{
        VecDeque,
        HashMap,
    },
    sync::{
        Arc,
    },
};

use serenity::{
    model::{
        id::{
            ChannelId,
            GuildId,
        },
        channel::Message,
    },
    prelude::*,
    Result as SerenityResult,
};

use serde_json::{
    Value,
    Map as JsonMap,
    Result as JsonResult,
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


use tracing::error;

pub mod play;
pub mod skip;
pub mod add;
pub mod pause;
pub mod stop;
pub mod queue;

pub struct MusicQueue;

impl TypeMapKey for MusicQueue{
    //Arc is the automatic reference counter that makes the memory safe in a rust env
    //Mutex allows access across threads with locking
    //HashMap has a double sided queue for each server that this bot is in
    //VecDeque is the double sided queue that allows us to push songs to the top and get the next
    //song from the bottom
    //SongInfo is the struct from the commands file
    type Value = Arc<Mutex<HashMap<GuildId, VecDeque<SongInfo>>>>;
}

pub struct CurrentSong;

impl TypeMapKey for CurrentSong{
    //Arc is the automatic reference counter that makes the memory safe in a rust env
    //Mutex allows access across threads with locking
    //HashMap has a tuple for each server that this bot is in
    //Option<Instant> is the moment the song started playing, None means the song is paused
    //SongInfo is the struct from the commands file
    type Value = Arc<Mutex<HashMap<GuildId, (Option<Instant>, SongInfo)>>>;
}

#[derive(Clone)]
pub struct SongInfo{
    pub json_map: JsonMap<String, Value>,
    pub channel: ChannelId,
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

pub fn check_msg(result: SerenityResult<Message>){
    if let Err(err) = result{
        error!("Failed to send message: {:?}", err);
    }
}
