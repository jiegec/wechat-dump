use std::{collections::HashMap, io::Write};

use clap::{App, Arg};
use sqlx::{Pool, Sqlite};
use std::{fs::File, path::Path};

async fn friends(root: &str) -> anyhow::Result<()> {
    let contacts = Path::new(root).join("WCDB_Contact.sqlite");
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

            if let Some(s) = remarks.get(&10) {
                writeln!(contact_file, "Nickname: {}", s)?;
            }
            if let Some(s) = remarks.get(&18) {
                writeln!(contact_file, "WeChat: {}", s)?;
            }
            if let Some(s) = remarks.get(&26) {
                writeln!(contact_file, "Contact Name: {}", s)?;
            }
            if let Some(s) = remarks.get(&66) {
                writeln!(contact_file, "Tags: {}", s)?;
            }

            let profiles = extract_tlv(profile);
            if let Some(s) = profiles.get(&18) {
                writeln!(contact_file, "Country: {}", s)?;
            }
            if let Some(s) = profiles.get(&26) {
                writeln!(contact_file, "State: {}", s)?;
            }
            if let Some(s) = profiles.get(&34) {
                writeln!(contact_file, "City: {}", s)?;
            }
            if let Some(s) = profiles.get(&42) {
                writeln!(contact_file, "Signature: {}", s)?;
            }
        }
    }
    Ok(())
}

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
    let matches = App::new("wechat-dump")
        .arg(
            Arg::with_name("ROOT")
                .required(true)
                .help("The root directory of Wechat files"),
        )
        .get_matches();
    let root = matches.value_of("ROOT").unwrap();
    friends(root).await?;
    Ok(())
}
