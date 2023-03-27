use chrono::NaiveDateTime;
use clap::{Arg, Command};
use indicatif::ProgressBar;
use sqlx::{Pool, Sqlite};
use std::{collections::HashMap, io::Write};
use std::{fs::File, path::Path};

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
            let remarks = extract_tlv(remark);
            if let Some(s) = remarks.get(&10) {
                writeln!(chatroom_file, "Name: {}", s)?;
            }

            // members
            if room.len() > 2 {
                let mut room_len = room[1] as usize;
                let mut offset = 2;
                if (room_len & 0x80) != 0 {
                    room_len = (room_len & 0x7F) + ((room[2] as usize) << 7);
                    offset = 3;
                }
                let xml = String::from_utf8_lossy(&room[offset..offset + room_len]).to_string();
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
                        }
                        writeln!(chatroom_file)?;

                        index += 1;
                    }
                }
            }
        } else {
            // contacts
            writeln!(contact_file, "\n## {}\n", name)?;
            let remarks = extract_tlv(remark);
            let mapping = [
                (10, "Nickname"),
                (18, "WeChat"),
                (26, "Contact Name"),
                (66, "Tags"),
            ];
            for (k, v) in mapping {
                if let Some(s) = remarks.get(&k) {
                    if !s.is_empty() {
                        writeln!(contact_file, "{}: {}", v, s)?;
                    }
                }
            }

            let profiles = extract_tlv(profile);
            let mapping = [
                (18, "Country"),
                (26, "State"),
                (34, "City"),
                (42, "Signature"),
            ];
            for (k, v) in mapping {
                if let Some(s) = profiles.get(&k) {
                    if !s.is_empty() {
                        writeln!(contact_file, "{}: {}", v, s)?;
                    }
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
                let time = NaiveDateTime::from_timestamp(create_time, 0);
                writeln!(message_file, "{:?} {}\n", time, msg)?;
                if ty == 1 && des == 0 {
                    writeln!(my_message_file, "{}", msg)?;
                }
            }
        }
    }
    Ok(())
}

// https://github.com/stomakun/WechatExport-iOS/blob/master/WechatExport/wechat.cs#L578
fn extract_tlv(data: &[u8]) -> HashMap<u8, String> {
    let mut res = HashMap::new();
    let mut offset = 0;
    while offset + 1 < data.len() {
        let tag = data[offset];

        // gender
        if tag == 0x08 && data[offset + 1] == 1 {
            res.insert(tag, String::from("Male"));
            offset += 2;
            continue;
        } else if tag == 0x08 && data[offset + 1] == 2 {
            res.insert(tag, String::from("Female"));
            offset += 2;
            continue;
        }

        let length = data[offset + 1];
        if offset + 2 + length as usize > data.len() {
            // unknown problem
            break;
        }
        let value =
            String::from_utf8_lossy(&data[offset + 2..offset + 2 + length as usize]).to_string();
        offset += 2 + length as usize;
        res.insert(tag, value);
    }
    res
}

#[async_std::main]
async fn main() -> anyhow::Result<()> {
    let matches = Command::new("wechat-dump")
        .arg(
            Arg::new("ROOT")
                .required(true)
                .help("The root directory of Wechat files"),
        )
        .get_matches();
    let root = matches.value_of("ROOT").unwrap();
    let name_map = match friends(root).await {
        Ok(name_map) => name_map,
        Err(err) => {
            eprintln!("Failed to dump friends: {}", err);
            HashMap::new()
        }
    };
    if let Err(err) = messages(root, &name_map).await {
        eprintln!("Failed to dump messages: {}", err);
    }
    Ok(())
}
