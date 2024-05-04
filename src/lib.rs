use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

// https://instagram-engineering.com/sharding-ids-at-instagram-1cf5a71e5a5c

const SEQUENCE_BIT_LEN: u64 = 10;
const SEQUENCE_BIT_MASK: u64 = (1 << SEQUENCE_BIT_LEN) - 1;
const MAX_SEQUENCES: u64 = 2 ^ SEQUENCE_BIT_LEN;
const LOGICAL_VOLUME_BIT_LEN: u64 = 13;
const LOGICAL_VOLUME_BIT_MASK: u64 = ((1 << LOGICAL_VOLUME_BIT_LEN) - 1) << SEQUENCE_BIT_LEN;
const TIME_BIT_LEN: u64 = 41;
// number of milliseconds since UTC epoch time
const JANUARY_1ST_2024_AS_DURATION: Duration = Duration::from_millis(1704067200541);

#[derive(Debug)]
pub enum Error {
    NoAvailableSequences,
}

pub struct Snowprint {
    settings: SnowprintSettings,
    state: SnowprintState,
}

// The point is to distribute ids across logical volume shards evenly
//     - reset sequence every MS to 0 to remain sortable
//     - increase logical volume sequence by 1 every MS
//     - return err if available logical volume ids have been used

// This assumes sequences + logical volume ids occur in the same ms

pub struct SnowprintSettings {
    pub origin_timestamp_ms: u64,
    pub logical_volume_modulo: u64,
    pub logical_volume_base: u64,
}

struct SnowprintState {
    pub origin_duration: Duration,
    pub last_duration_ms: u64,
    pub sequence_id: u64,
    pub logical_volume_id: u64,
    pub last_logical_volume_id: u64,
}

impl Snowprint {
    pub fn new(settings: SnowprintSettings) -> Snowprint {
        let origin_duration = Duration::from_millis(settings.origin_timestamp_ms);
        let duration_ms = match SystemTime::now().duration_since(UNIX_EPOCH + origin_duration) {
            // check time didn't go backward
            Ok(duration) => duration.as_millis() as u64,
            // time went backwards so use the most recent step
            _ => {
                println!("ops went backwards!");
                settings.origin_timestamp_ms
            }
        };

        Snowprint {
            settings: settings,
            state: SnowprintState {
                origin_duration: origin_duration,
                last_duration_ms: duration_ms,
                sequence_id: 0,
                logical_volume_id: 0,
                last_logical_volume_id: 0,
            },
        }
    }

    pub fn get_snowprint(&mut self) -> Result<u64, Error> {
        let duration_ms =
            match SystemTime::now().duration_since(UNIX_EPOCH + self.state.origin_duration) {
                // check time didn't go backward
                Ok(duration) => {
                    let dur_ms = duration.as_millis() as u64;
                    match dur_ms > self.state.last_duration_ms {
                        true => dur_ms,
                        _ => self.state.last_duration_ms,
                    }
                }
                // time went backwards so use the most recent step
                _ => self.state.last_duration_ms,
            };

        compose_snowprint_from_settings_and_state(&mut self.state, &self.settings, duration_ms)
    }
}

fn compose_snowprint_from_settings_and_state(
    state: &mut SnowprintState,
    settings: &SnowprintSettings,
    duration_ms: u64,
) -> Result<u64, Error> {
    // time changed
    if state.last_duration_ms < duration_ms {
        state.last_duration_ms = duration_ms;
        state.sequence_id = 0;
        state.last_logical_volume_id = state.logical_volume_id;
        state.logical_volume_id = (state.logical_volume_id + 1) % settings.logical_volume_modulo;
    } else {
        // time did not change!
        state.sequence_id += 1;
        if state.sequence_id > MAX_SEQUENCES - 1 {
            let next_logical_volume_id =
                (state.logical_volume_id + 1) % settings.logical_volume_modulo;
            // cycled through all sequences on all available logical shards
            if next_logical_volume_id == state.last_logical_volume_id {
                return Err(Error::NoAvailableSequences);
            }
            // move to next shard
            state.sequence_id = 0;
            state.logical_volume_id = next_logical_volume_id;
        }
    }

    Ok(compose_snowprint(
        duration_ms as u64,
        settings.logical_volume_base + state.logical_volume_id,
        state.sequence_id,
    ))
}

// at it's core this is a snowprint
pub fn compose_snowprint(ms_timestamp: u64, logical_id: u64, ticket_id: u64) -> u64 {
    ms_timestamp << (LOGICAL_VOLUME_BIT_LEN + SEQUENCE_BIT_LEN)
        | logical_id << SEQUENCE_BIT_LEN
        | ticket_id
}

pub fn decompose_snowprint(snowprint: u64) -> (u64, u64, u64) {
    let time = snowprint >> (LOGICAL_VOLUME_BIT_LEN + SEQUENCE_BIT_LEN);
    let logical_id = (snowprint & LOGICAL_VOLUME_BIT_MASK) >> SEQUENCE_BIT_LEN;
    let ticket_id = snowprint & SEQUENCE_BIT_MASK;

    (time, logical_id, ticket_id)
}