use chrono::NaiveDateTime;
use clap::Parser;
use indicatif::ProgressBar;
use prost::Message;
use sqlx::{Pool, Sqlite};
use std::{collections::HashMap, io::Write};
use std::{fs::File, path::Path};

include!(concat!(env!("OUT_DIR"), "/wechat.dump.rs"));

async fn friends(root: &str) -> anyhow::Result<HashMap<String, String>> {
    // map user name hash to user name
    let mut name_map = HashMap::new();
    let contacts = Path::new(root).join("WCDB_Contact.sqlite");
    println!("Opening {}", contacts.display());
    let pool = Pool::<Sqlite>::connect(&format!("sqlite:{}", contacts.display())).await?;
    let friends: Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT userName, dbContactRemark, dbContactProfile, dbContactChatRoom FROM Friend ORDER BY userName",
    )
    .fetch_all(&pool)
    .await?;

    println!("Saving {} friends", friends.len());
    let mut contact_file = File::create("contacts.md")?;
    writeln!(contact_file, "# Contacts\n")?;
    let mut chatroom_file = File::create("chatrooms.md")?;
    writeln!(chatroom_file, "# Contacts\n")?;
    for (name, remark, profile, room) in &friends {
        // MD5(name) => name
        let digest = md5::compute(name.as_bytes());
        name_map.insert(format!("{:x}", digest), name.clone());

        if name.ends_with("@chatroom") {
            // chat rooms
            writeln!(chatroom_file, "\n## {}\n", name)?;
            if let Ok(remark) = Remark::decode(remark.as_slice()) {
                writeln!(chatroom_file, "Name: {}", remark.nickname)?;
            }

            // members
            if let Ok(chatroom) = Chatroom::decode(room.as_slice()) {
                let xml = chatroom.room_info_xml;
                if let Ok(doc) = roxmltree::Document::parse(&xml) {
                    let root = doc.root_element();
                    writeln!(chatroom_file, "Members:")?;
                    let mut index = 0;
                    for member in root.children() {
                        if let Some(user_name) = member.attribute("UserName") {
                            write!(chatroom_file, "{}: {}", index, user_name)?;
                        }
                        for e in member.children() {
                            if e.tag_name().name() == "InviterUserName" {
                                if let Some(inviter) = e.text() {
                                    write!(chatroom_file, " invited by {}", inviter)?;
                                }
                            }
                            if e.tag_name().name() == "DisplayName" {
                                if let Some(display_name) = e.text() {
                                    write!(chatroom_file, " ({})", display_name)?;
                                }
                            }
                        }
                        writeln!(chatroom_file)?;

                        index += 1;
                    }
                }
            }
        } else {
            // contacts
            writeln!(contact_file, "\n## {}\n", name)?;
            if let Ok(remark) = Remark::decode(remark.as_slice()) {
                if !remark.nickname.is_empty() {
                    writeln!(contact_file, "Nickname: {}", remark.nickname)?;
                }
                if !remark.wechat.is_empty() {
                    writeln!(contact_file, "WeChat ID: {}", remark.wechat)?;
                }
                if !remark.alias.is_empty() {
                    writeln!(contact_file, "Alias: {}", remark.alias)?;
                }
                if !remark.tags.is_empty() {
                    writeln!(contact_file, "Tags: {}", remark.tags)?;
                }
            }

            if let Ok(profile) = Profile::decode(profile.as_slice()) {
                if profile.gender != 0 {
                    writeln!(
                        contact_file,
                        "Gender: {}",
                        if profile.gender == 1 {
                            "Male"
                        } else if profile.gender == 2 {
                            "Female"
                        } else {
                            "Others"
                        }
                    )?;
                }
                if !profile.country.is_empty() {
                    writeln!(contact_file, "Country: {}", profile.country)?;
                }
                if !profile.state.is_empty() {
                    writeln!(contact_file, "State: {}", profile.state)?;
                }
                if !profile.city.is_empty() {
                    writeln!(contact_file, "City: {}", profile.city)?;
                }
                if !profile.signature.is_empty() {
                    writeln!(contact_file, "Signature: {}", profile.signature)?;
                }
            }
        }
    }
    Ok(name_map)
}

async fn messages(root: &str, name_map: &HashMap<String, String>) -> anyhow::Result<()> {
    let mut message_file = File::create("messages.md")?;
    writeln!(message_file, "# Messages\n")?;
    let mut my_message_file = File::create("my_messages.md")?;
    for index in 1.. {
        let contacts = Path::new(root).join(format!("message_{}.sqlite", index));
        if !contacts.exists() {
            break;
        }
        println!("Opening {}", contacts.display());

        let pool = Pool::<Sqlite>::connect(&format!("sqlite:{}", contacts.display())).await?;
        let tables: Vec<(String, String)> = sqlx::query_as(
            "SELECT type, name FROM sqlite_master WHERE type = 'table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await?;
        println!(
            "Found {} tables in file message_{}.sqlite",
            tables.len(),
            index
        );

        let pb = ProgressBar::new(tables.len() as u64);
        for (_ty, table) in tables {
            pb.inc(1);

            if !table.starts_with("Chat_") {
                continue;
            }
            let messages: Vec<(i64, i64, i64, String)> = sqlx::query_as(&format!(
                "SELECT CreateTime, Type, Des, Message FROM {} ORDER BY CreateTime",
                table
            ))
            .fetch_all(&pool)
            .await?;
            let title = table
                .strip_prefix("Chat_")
                .and_then(|name| name_map.get(name))
                .unwrap_or(&table);
            writeln!(message_file, "\n## {}\n", title)?;

            for (create_time, ty, des, message) in messages {
                // https://github.com/BlueMatthew/WechatExporter/blob/f9685ba6cc1932bb6f08c465cd2c4eda769538e0/WechatExporter/core/MessageParser.cpp#L58
                // https://github.com/ppwwyyxx/wechat-dump/blob/master/wechat/msg.py
                let msg = match ty {
                    // text message
                    1 => message,
                    3 => format!("Image"),
                    34 => format!("Voice"),
                    42 => format!("Share User"),
                    43 => format!("Video"),
                    47 => format!("Emoji"),
                    48 => format!("Location"),
                    49 => format!("App Message"),
                    50 => format!("Voice Call"),
                    // recall
                    10000 => message,
                    10002 => format!("System Message"),
                    _ => format!("Unknown message type: {}", ty),
                };
                let time = NaiveDateTime::from_timestamp_opt(create_time, 0).unwrap();
                writeln!(message_file, "{:?} {}\n", time, msg)?;
                if ty == 1 && des == 0 {
                    writeln!(my_message_file, "{}", msg)?;
                }
            }
        }
    }
    Ok(())
}

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The root directory of Wechat files
    root: String,
}

#[async_std::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let name_map = match friends(&cli.root).await {
        Ok(name_map) => name_map,
        Err(err) => {
            eprintln!("Failed to dump friends: {}", err);
            HashMap::new()
        }
    };
    if let Err(err) = messages(&cli.root, &name_map).await {
        eprintln!("Failed to dump messages: {}", err);
    }
    Ok(())
}
