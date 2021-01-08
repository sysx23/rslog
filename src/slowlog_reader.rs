use crate::slowlog::SlowlogRecord;

pub struct SlowlogReader {
    client: redis::Client,
    con: redis::Connection,
    last_id: i64,
    length: u32,
    uptime: u64,
}

impl std::convert::TryFrom<redis::Client> for SlowlogReader {
    type Error = redis::RedisError;
    fn try_from(client: redis::Client) -> Result<Self, Self::Error> {
        let sl_reader = SlowlogReader {
            con: client.get_connection()?,
            client: client,
            last_id: -1,
            length: 128,
            uptime: 0,
        };
        Ok(sl_reader)
    }
}

pub fn get_slowlog(
    con: &mut redis::Connection,
    length: u32,
) -> redis::RedisResult<Vec<SlowlogRecord>> {
    log::debug!("Executing slowlog query");
    redis::cmd("SLOWLOG").arg("GET").arg(length).query(con)
}

fn get_uptime(con: &mut redis::Connection) -> redis::RedisResult<u64> {
    let server_info = redis::cmd("INFO").arg("SERVER").query::<String>(con)?;
    server_info
        .lines()
        .filter(|l| l.contains("uptime_in_seconds"))
        .nth(0)
        .ok_or((
            redis::ErrorKind::TypeError,
            "No uptime line in response from server",
        ))?
        .split(':')
        .nth(1)
        .ok_or((
            redis::ErrorKind::TypeError,
            "No value for uptime in response from server",
        ))?
        .parse::<u64>()
        .map_err(|e: std::num::ParseIntError| {
            redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Error while trying to parse uptime from response",
                e.to_string(),
            ))
        })
}

impl SlowlogReader {
    pub fn get(&mut self) -> redis::RedisResult<Vec<SlowlogRecord>> {
        self.check_for_restart()?;
        let new_records: Vec<_> = get_slowlog(&mut self.con, self.length)?
            .into_iter()
            .filter(|r| r.id as i64 > self.last_id)
            .collect();
        self.last_id = new_records.get(0).map_or(self.last_id, |r| r.id as i64);
        Ok(new_records)
    }
    pub fn update_connection(&mut self) -> Result<(), redis::RedisError> {
        self.con = self.client.get_connection()?;
        Ok(())
    }

    fn check_for_restart(&mut self) -> redis::RedisResult<()> {
        let uptime = get_uptime(&mut self.con)?;
        if uptime < self.uptime {
            self.last_id = -1
        }
        self.uptime = uptime;
        Ok(())
    }

    pub fn redis_error_handler(&mut self, e: redis::RedisError) -> Result<(), redis::RedisError> {
        if matches!(e.kind(), redis::ErrorKind::IoError) {
            log::warn!(
                "Lost connection to redis cluster, trying to establish a new one. Error: {}",
                e
            );
            if let Err(e) = self.update_connection() {
                return Err(e);
            }
        }
        Ok(())
    }
}
