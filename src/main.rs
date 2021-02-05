use std::{collections::HashMap, io::Write};

use clap::{App, Arg};
use sqlx::{Pool, Sqlite};
use std::{fs::File, path::Path, prelude::*};

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
    let contacts = Path::new(root).join("WCDB_Contact.sqlite");
    let pool = Pool::<Sqlite>::connect(&format!("sqlite:{}", contacts.display())).await?;
    let friends: Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT userName, dbContactRemark, dbContactProfile, dbContactChatRoom FROM Friend ORDER BY userName",
    )
    .fetch_all(&pool)
    .await?;

    println!("Saving {} contacts", friends.len());
    let mut contact_file = File::create("contacts.md")?;
    writeln!(contact_file, "# Contacts\n")?;
    for (name, remark, profile, room) in &friends {
        // ignore chat rooms
        if name.ends_with("@chatroom") {
            continue;
        }

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
    Ok(())
}
