use std::{
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
};


use tracing::error;

pub mod boombox;

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

pub fn check_msg(result: SerenityResult<Message>){
    if let Err(err) = result{
        error!("Failed to send message: {:?}", err);
    }
}
