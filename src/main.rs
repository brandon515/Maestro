mod commands;

use std::{
    env,
    sync::{
        Arc,
    },
    collections::{
        HashSet,
        VecDeque,
        HashMap,
    },
    time::{
        Instant,
        Duration,
    },
};

use serenity::{
    async_trait,
    client::bridge::gateway::ShardManager,
    framework::{
        StandardFramework,
        standard::{
            macros::{
                group,
                hook,
            },
            CommandResult,
        },
    },
    http::Http,
    model::{
        event::ResumedEvent, 
        gateway::Ready,
        channel::Message,
        id::GuildId,
    },
    prelude::*,
};

use songbird::{
    SerenityInit,
    Songbird,
};

use tracing::{error, info};
use tracing_subscriber::{
    FmtSubscriber,
    EnvFilter,
};

use commands::{
    boombox::*,
    SongInfo,
    check_msg,
    MusicQueue,
    CurrentSong,
};



#[hook]
async fn before(_ctx: &Context, msg: &Message, command_name: &str) -> bool {
    info!("Got command '{}' by User '{}'", command_name, msg.author.name);
    true
}

#[hook]
async fn after(_ctx: &Context, _msg: &Message, command_name: &str, command_result: CommandResult){
    match command_result {
        Ok(()) => info!("Processed command '{}'", command_name),
        Err(err) => error!("Command '{}' returned error {:?}", command_name, err),
    }
}

pub struct ShardManagerContainer;

impl TypeMapKey for ShardManagerContainer{
    type Value = Arc<Mutex<ShardManager>>;
}

struct Handler;

#[async_trait]
impl EventHandler for Handler{
    async fn ready(&self, _: Context, ready: Ready){
        info!("Connected as {}", ready.user.name);
    }

    async fn resume(&self, _: Context, _: ResumedEvent) {
        info!("Resumed");
    }
}

#[group]
#[commands(play, mechanicus, skip, skipto, add, pause, stop, queue)]
struct General;

#[tokio::main]
async fn main() {
    dotenv::dotenv().expect("failed to load .env file");

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to start the logger");

    let token = env::var("DISCORD_TOKEN")
        .expect("Expected a token in the enviroment");

    let http = Http::new_with_token(&token);
    let thread_http = Http::new_with_token(&token);

    // Use the discord api to find the bot's owners and ID
    let (owners, _bot_id) = match http.get_current_application_info().await {
        Ok(info) => {
            // HashSets are just lists that promise there are not duplicate values
            let mut owners = HashSet::new();
            owners.insert(info.owner.id);

            (owners, info.id)
        },
        Err(err) => panic!("Could not access application info: {:?}", err),
    };

    let sb = Arc::new(Songbird::serenity()); // this should be a mutex but the serenity client can't handle that... oh well

    let framework = StandardFramework::new()
        .configure(|c| c
            .owners(owners)
            .prefix("!")) //command prefix
        .before(before) //the function to run before all commands, don't use this to validate whether a command should be run
        .after(after) 
        .group(&GENERAL_GROUP); //all commands given to the general struct up there

    let mut client = Client::builder(&token)
        .framework(framework)
        .event_handler(Handler)
        .register_songbird_with(Arc::clone(&sb))
        .await
        .expect("Error creating client");

    let music_queue = Arc::new(Mutex::new(HashMap::<GuildId, VecDeque<SongInfo>>::new()));
    let current_song = Arc::new(Mutex::new(HashMap::<GuildId, (Option<Instant>, SongInfo)>::new()));
    let shard_manager = client.shard_manager.clone();

    let mut data = client.data.write().await; // Data to be shared across all the commands
    data.insert::<ShardManagerContainer>(client.shard_manager.clone());
    data.insert::<MusicQueue>(music_queue.clone()); 
    data.insert::<CurrentSong>(current_song.clone());
    drop(data);


    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("ctrl-c register didn't get register");
        shard_manager.lock().await.shutdown_all().await;
    });

    // The thread that monitors the music queue and plays the next song where applicable
    tokio::spawn(async move {
        loop{
            for (serv, (pos_ins, song)) in current_song.lock().await.iter_mut(){
                // if there's None here that means the queue is paused so don't do anything
                if let Some(ins) = pos_ins{
                    // ins is the moment the song started playing
                    let song_dur = Duration::from_secs(song.json_map.get("duration").and_then(serde_json::Value::as_u64).unwrap());
                    if ins.elapsed() >= song_dur.checked_add(Duration::from_secs(10)).unwrap(){ //adds a buffer in between songs, less jarring this way
                        let mut mq = music_queue.lock().await;
                        let q = mq.entry(serv.clone()).or_insert(VecDeque::new());
                        // get the next song if it's there, if not than we ran through the queue
                        // and we do nothing 
                        if let Some(next_song) = q.pop_front(){
                            match sb.get(serv.clone()){// Songbird instance from the main thread
                                Some(handler_lock) => {
                                    let mut handler = handler_lock.lock().await;

                                    let source = match commands::boombox::make_source(&next_song){
                                        Some(src) => src,
                                        None => {
                                            error!("Failed to play the next song");
                                            check_msg(next_song.channel.say(&thread_http, "Can't play the next queued song").await);
                                            continue;
                                        }
                                    };

                                    handler.play_only_source(source);
                                    check_msg(next_song.channel.say(&thread_http, &format!("Playing {}", next_song.json_map.get("title").and_then(serde_json::Value::as_str).unwrap())).await);
                                    *pos_ins = Some(Instant::now());
                                    *song = next_song;
                                },
                                // we've been disconnected somehow, put the stream on pause
                                None => {
                                    *pos_ins = None
                                },
                            };
                        }
                    }
                }
            }
        }
    });

    if let Err(err) = client.start().await {
        error!("Client error: {:?}", err);
    }

}
