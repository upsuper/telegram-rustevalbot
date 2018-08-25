use serde_json;
use std::collections::VecDeque;
use std::fs::File;
use std::io;
use telegram_bot::MessageId;

const RECORD_LIST_FILE: &str = "record_list.json";

pub struct RecordService(VecDeque<Record>);

impl RecordService {
    /// Create record list, restore from record list file if possible.
    pub fn init() -> Self {
        match File::open(RECORD_LIST_FILE) {
            Ok(file) => match serde_json::from_reader(file) {
                Ok(list) => return RecordService(list),
                Err(e) => error!("failed to parse record list: {:?}", e),
            },
            Err(e) => {
                // It's fine that the file doesn't exist.
                if e.kind() != io::ErrorKind::NotFound {
                    error!("failed to read record list: {:?}", e);
                }
            }
        }
        RecordService(Default::default())
    }

    /// Push a new record with reply being empty.
    pub fn push_record(&mut self, msg: MessageId, date: i64) {
        let reply = None;
        self.0.push_back(Record { msg, reply, date });
    }

    fn find_record(&self, msg: MessageId) -> Option<&Record> {
        self.0.iter().rev().find(|r| r.msg == msg)
    }

    fn find_record_mut(&mut self, msg: MessageId) -> Option<&mut Record> {
        self.0.iter_mut().rev().find(|r| r.msg == msg)
    }

    /// Find the reply message of the given record.
    pub fn find_reply(&self, msg: MessageId) -> Option<MessageId> {
        self.find_record(msg).and_then(|r| r.reply)
    }

    /// Set the reply message of the given record.
    pub fn set_reply(&mut self, msg: MessageId, reply: MessageId) {
        self.find_record_mut(msg).map(|r| r.reply = Some(reply));
    }

    /// Remove the reply message of the given record.
    pub fn remove_reply(&mut self, msg: MessageId) {
        self.find_record_mut(msg).map(|r| r.reply = None);
    }

    /// Clear records order than 48hrs before the given date.
    pub fn clear_old_records(&mut self, current_date: i64) {
        // We can clean up records up to 48hrs ago, because messages before that
        // cannot be edited anymore.
        let date_to_clean = current_date - 48 * 3600;
        while let Some(record) = self.0.pop_front() {
            if record.date > date_to_clean {
                self.0.push_front(record);
                break;
            }
        }
    }
}

impl Drop for RecordService {
    fn drop(&mut self) {
        match File::create(RECORD_LIST_FILE) {
            Ok(file) => match serde_json::to_writer(file, &self.0) {
                Ok(()) => {}
                Err(e) => error!("failed to serialize record list: {:?}", e),
            },
            Err(e) => error!("failed to create record list: {:?}", e),
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Record {
    msg: MessageId,
    reply: Option<MessageId>,
    /// Same as Message::date, a UNIX epoch in seconds.
    date: i64,
}
