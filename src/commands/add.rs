use tracing::{error, info};
use crate::commands::{
    check_msg,
    MusicQueue,
    pull_youtube_child,
    process_output,
};
use std::{
    io::{
        BufRead,
        BufReader,
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
