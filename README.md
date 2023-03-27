# wechat-dump

Dump wechat messages into readable format.

Steps:

1. Backup iPhone via iMazing

2. Extract files from iMazing backup via File System -> Backup -> Apps -> AppDomain-com.tencent.xin -> Documents/uid/DB

- message_*.sqlite
- MM.sqlite
- WCDB_Contact.sqlite
- WCDB_OpLog.sqlite

Copy them to Mac.

3. Run this program with the folder containing files above:

```shell
cargo run -- /path/to/wechat/db
```